//! 一等公民 `AndroidAdapter`：把已封存的 `AuthorizedAction` 经请求/响应协议
//! 转发到设备侧 localhost bridge，并把设备的真实执行结果如实映射为
//! `ActionOutcome` / `AdapterError`。
//!
//! 与旧实现（`DefaultActionExecutor` 内部 env 分支 + fire-and-forget）相比的关键差异：
//!
//! 1. **诚实的 outcome**：请求/响应。转发后**读取**设备回执；超时、连接被拒、设备
//!    拒绝或回执非法都返回 `Err(AdapterError::AndroidBridgeError)` → 生命周期记为
//!    `Failed`，而不再把"一次成功的 TCP 写"谎报为 `Succeeded`。
//! 2. **注入式配置**：[`AndroidBridgeConfig`] 在**构造时**注入，`execute()` 内不再
//!    读取进程环境变量 —— 移除执行期的 ambient authority。
//! 3. **HMAC 绑定**：认证标签覆盖 freshness window 与 `AuthorizedAction` 字节，
//!    而非静态 bearer token，使捕获到的标签无法重放到另一个 action 或过期窗口。
//!
//! 非转发类动作（或缺少有效目标的 `PrefetchFile`）回退到内部
//! [`DefaultActionExecutor`] 的确定性本地 stub —— 复用同一套 stub 语义，不重复实现。
//!
//! 注意：`AndroidAdapter` **不**进入 golden/replay 的 hash 路径（那条路径用
//! `OfflineAdapter`）。设备侧 latency/成败本就非确定，本 adapter 只产生供检视的
//! 审计轨迹，不参与可复现性断言。

use std::env;
use std::io::{ErrorKind, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use aios_core::governance::{ActionAdapter, AuthorizedAction};
use aios_spec::bridge::{
    BridgeAuth, BridgeExecuteRequest, BridgeExecuteResponse, BridgeStatus,
    BRIDGE_MESSAGE_TYPE_EXECUTE,
};
use aios_spec::governance::{ActionOutcome, AdapterError};
use aios_spec::intent::{ActionType, SuggestedAction};
use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::DefaultActionExecutor;

type HmacSha256 = Hmac<Sha256>;

const DEFAULT_ANDROID_ACTION_BRIDGE_PORT: u16 = 46321;
const ANDROID_ACTION_PAYLOAD_TTL_MS: i64 = 60_000;
const READ_TIMEOUT_MS: u64 = 5000;
const MAX_RESPONSE_BYTES: usize = 4096;

/// 转发到 Android bridge 所需的配置。构造时注入，不在 `execute()` 内读 env。
#[derive(Debug, Clone)]
pub struct AndroidBridgeConfig {
    pub host: String,
    pub port: u16,
    /// HMAC-SHA256 共享密钥（key），转发时必需。
    pub auth_key: Option<String>,
}

impl AndroidBridgeConfig {
    /// 从环境变量构造配置。**仅供启动期**调用一次（如 daemon 装配 adapter 时），
    /// 不应在每次动作执行时调用。返回 `None` 表示未启用 Android bridge。
    pub fn from_env() -> Option<Self> {
        if !env_flag("DIPECS_ANDROID_ACTION_BRIDGE_ENABLED") {
            return None;
        }

        let host = env::var("DIPECS_ANDROID_ACTION_BRIDGE_HOST")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "127.0.0.1".to_string());
        let port = env::var("DIPECS_ANDROID_ACTION_BRIDGE_PORT")
            .ok()
            .and_then(|value| value.parse::<u16>().ok())
            .unwrap_or(DEFAULT_ANDROID_ACTION_BRIDGE_PORT);
        let auth_key = env::var("DIPECS_ANDROID_ACTION_BRIDGE_TOKEN")
            .ok()
            .filter(|value| !value.trim().is_empty());
        Some(Self {
            host,
            port,
            auth_key,
        })
    }
}

/// 转发到设备侧 bridge 的一等 adapter。
pub struct AndroidAdapter {
    config: AndroidBridgeConfig,
    /// 非转发类动作的本地回退执行器（确定性 stub）。
    fallback: DefaultActionExecutor,
}

impl AndroidAdapter {
    pub fn new(config: AndroidBridgeConfig) -> Self {
        Self {
            config,
            fallback: DefaultActionExecutor::new(),
        }
    }

    /// 把动作转发到设备并把回执映射为 outcome。任何失败均为 `Err` → `Failed`。
    fn forward(
        &self,
        authorized: &AuthorizedAction,
        target: &str,
    ) -> Result<ActionOutcome, AdapterError> {
        let auth_key = self.config.auth_key.as_deref().ok_or_else(|| {
            AdapterError::AndroidBridgeError(
                "DIPECS_ANDROID_ACTION_BRIDGE_TOKEN is required when forwarding to Android bridge"
                    .into(),
            )
        })?;

        // canonical 序列化 AuthorizedAction；HMAC 绑定这些字节和 freshness window。
        let action_json = serde_json::to_string(authorized).map_err(|error| {
            AdapterError::AndroidBridgeError(format!("serialize AuthorizedAction: {error}"))
        })?;
        let issued_at_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| {
                AdapterError::AndroidBridgeError(format!(
                    "calculate Android bridge payload issue time: {error}"
                ))
            })?
            .as_millis() as i64;
        let expires_at_ms = issued_at_ms + ANDROID_ACTION_PAYLOAD_TTL_MS;
        let canonical = canonical_execute_envelope_input(issued_at_ms, expires_at_ms, &action_json);
        let hmac_sha256 = compute_hmac(auth_key, canonical.as_bytes())
            .map_err(AdapterError::AndroidBridgeError)?;

        let request = BridgeExecuteRequest {
            message_type: BRIDGE_MESSAGE_TYPE_EXECUTE.to_string(),
            issued_at_ms,
            expires_at_ms,
            auth: BridgeAuth { hmac_sha256 },
            action: action_json,
        };
        let payload = serde_json::to_string(&request).map_err(|error| {
            AdapterError::AndroidBridgeError(format!("serialize bridge request: {error}"))
        })?;

        let raw = send_request(&self.config.host, self.config.port, payload.as_bytes())
            .map_err(AdapterError::AndroidBridgeError)?;
        let response: BridgeExecuteResponse = serde_json::from_slice(&raw).map_err(|error| {
            AdapterError::AndroidBridgeError(format!("parse bridge response: {error}"))
        })?;

        match response.status {
            BridgeStatus::Ok => {
                tracing::info!(
                    host = %self.config.host,
                    port = self.config.port,
                    target = %target,
                    "AndroidAdapter: device confirmed execution"
                );
                Ok(ActionOutcome {
                    action_type: format!("{:?}", authorized.action().action_type),
                    target: authorized.action().target.clone(),
                    summary: response
                        .summary
                        .unwrap_or_else(|| "android_executed".to_string()),
                    latency_us: response.latency_us.unwrap_or(0),
                })
            },
            BridgeStatus::Rejected => Err(AdapterError::AndroidBridgeError(format!(
                "android bridge rejected action: {}",
                response
                    .error
                    .unwrap_or_else(|| "no reason given".to_string())
            ))),
            BridgeStatus::Error => Err(AdapterError::AndroidBridgeError(format!(
                "android bridge execution error: {}",
                response
                    .error
                    .unwrap_or_else(|| "no detail given".to_string())
            ))),
        }
    }
}

impl ActionAdapter for AndroidAdapter {
    fn name(&self) -> &'static str {
        "android"
    }

    fn execute(&self, authorized: &AuthorizedAction) -> Result<ActionOutcome, AdapterError> {
        match classify(authorized.action()) {
            Route::Forward(target) => self.forward(authorized, target),
            Route::Local => self.fallback.execute(authorized),
        }
    }
}

/// 路由判定：
///   - `PrefetchFile`：仅带 `url:`/`uri:` 目标时转发到设备。
///   - `PreWarmProcess` / `KeepAlive` / `ReleaseMemory`：无条件转发到设备；
///     设备侧 bridge 解析 `AuthorizedAction` 中内嵌的 `action_type` + `target`
///     并执行结构化动作。
///   - `NoOp` 及其它未知类型留在本地 stub。
enum Route<'a> {
    Forward(&'a str),
    Local,
}

fn classify(action: &SuggestedAction) -> Route<'_> {
    match action.action_type {
        ActionType::PrefetchFile => match action.target.as_deref() {
            Some(target) if target.starts_with("url:") || target.starts_with("uri:") => {
                Route::Forward(target)
            },
            _ => Route::Local,
        },
        ActionType::PreWarmProcess | ActionType::KeepAlive | ActionType::ReleaseMemory => {
            Route::Forward(action.target.as_deref().unwrap_or(""))
        },
        _ => Route::Local,
    }
}

/// Canonical HMAC input shared with the Android bridge.
fn canonical_execute_envelope_input(
    issued_at_ms: i64,
    expires_at_ms: i64,
    action_json: &str,
) -> String {
    format!(
        "dipecs.android.bridge.execute.v1\nissued_at_ms:{issued_at_ms}\nexpires_at_ms:{expires_at_ms}\naction:{}:{action_json}",
        action_json.len(),
    )
}

/// 对 canonical 消息字节计算 HMAC-SHA256，返回小写 hex。
fn compute_hmac(key: &str, message: &[u8]) -> Result<String, String> {
    let mut mac = HmacSha256::new_from_slice(key.as_bytes())
        .map_err(|error| format!("init HMAC key: {error}"))?;
    mac.update(message);
    Ok(hex::encode(mac.finalize().into_bytes()))
}

/// 建连、写请求、半关写端发 EOF、读到 EOF/完整 JSON/超时为止。
///
/// 这是"诚实"的核心：超时且无数据、连接被拒、对端空回都映射为 `Err`，绝不静默成功。
fn send_request(host: &str, port: u16, payload: &[u8]) -> Result<Vec<u8>, String> {
    let mut stream = TcpStream::connect((host, port))
        .map_err(|error| format!("connect Android bridge {host}:{port}: {error}"))?;
    stream
        .set_read_timeout(Some(Duration::from_millis(READ_TIMEOUT_MS)))
        .map_err(|error| format!("set read timeout: {error}"))?;
    stream
        .write_all(payload)
        .map_err(|error| format!("write request to {host}:{port}: {error}"))?;
    stream
        .flush()
        .map_err(|error| format!("flush request to {host}:{port}: {error}"))?;
    // 半关写端：让按 EOF 读取的服务端知道请求已完整，立即回执。
    stream
        .shutdown(Shutdown::Write)
        .map_err(|error| format!("shutdown write to {host}:{port}: {error}"))?;

    let mut buf = Vec::with_capacity(MAX_RESPONSE_BYTES);
    let mut chunk = [0u8; 1024];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                if buf.len() + n > MAX_RESPONSE_BYTES {
                    return Err(format!(
                        "bridge response exceeded {MAX_RESPONSE_BYTES} bytes"
                    ));
                }
                buf.extend_from_slice(&chunk[..n]);
                // 已能解析出完整 JSON 即可停（服务端可能保持半开）。
                if serde_json::from_slice::<serde_json::Value>(&buf).is_ok() {
                    break;
                }
            },
            Err(error)
                if error.kind() == ErrorKind::WouldBlock || error.kind() == ErrorKind::TimedOut =>
            {
                if buf.is_empty() {
                    return Err(format!("read bridge response from {host}:{port}: {error}"));
                }
                break;
            },
            Err(error) => {
                return Err(format!("read bridge response from {host}:{port}: {error}"));
            },
        }
    }

    if buf.is_empty() {
        return Err(format!("bridge {host}:{port} closed without response"));
    }
    Ok(buf)
}

fn env_flag(name: &str) -> bool {
    matches!(
        env::var(name).ok().as_deref(),
        Some("1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON")
    )
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::{Shutdown, TcpListener};
    use std::thread;

    use aios_spec::intent::ActionUrgency;

    use super::*;

    #[test]
    fn env_flag_accepts_true_values() {
        assert!(env_flag_eval("true"));
        assert!(env_flag_eval("1"));
        assert!(env_flag_eval("ON"));
        assert!(!env_flag_eval("false"));
        assert!(!env_flag_eval(""));
    }

    fn env_flag_eval(value: &str) -> bool {
        std::env::set_var("DIPECS_TEST_FLAG", value);
        let enabled = env_flag("DIPECS_TEST_FLAG");
        std::env::remove_var("DIPECS_TEST_FLAG");
        enabled
    }

    #[test]
    fn hmac_is_deterministic_and_key_sensitive() {
        let a = compute_hmac("k1", b"payload").unwrap();
        let b = compute_hmac("k1", b"payload").unwrap();
        let c = compute_hmac("k2", b"payload").unwrap();
        let d = compute_hmac("k1", b"payload2").unwrap();
        assert_eq!(a, b, "same key+message must yield same tag");
        assert_ne!(a, c, "different key must change tag");
        assert_ne!(a, d, "different message must change tag");
        assert_eq!(a.len(), 64, "SHA-256 HMAC hex is 64 chars");
    }

    fn prefetch(target: Option<&str>) -> SuggestedAction {
        SuggestedAction {
            action_type: ActionType::PrefetchFile,
            target: target.map(|s| s.to_string()),
            urgency: ActionUrgency::Immediate,
        }
    }

    #[test]
    fn classify_routes_url_prefetch_to_forward() {
        assert!(matches!(
            classify(&prefetch(Some("url:https://x.test/a"))),
            Route::Forward(_)
        ));
        assert!(matches!(
            classify(&prefetch(Some("uri:content://x"))),
            Route::Forward(_)
        ));
    }

    #[test]
    fn classify_routes_non_forwardable_to_local() {
        // PrefetchFile without an Android target stays local.
        assert!(matches!(
            classify(&prefetch(Some("file:/tmp/x"))),
            Route::Local
        ));
        assert!(matches!(classify(&prefetch(None)), Route::Local));
        // NoOp (and any truly unknown type) stays local.
        let noop = SuggestedAction {
            action_type: ActionType::NoOp,
            target: None,
            urgency: ActionUrgency::Immediate,
        };
        assert!(matches!(classify(&noop), Route::Local));
    }

    #[test]
    fn classify_routes_forwarded_actions() {
        // PreWarmProcess, KeepAlive, ReleaseMemory always forward.
        for (action_type, target) in [
            (ActionType::PreWarmProcess, Some("pkg:com.example")),
            (ActionType::PreWarmProcess, None),
            (ActionType::KeepAlive, Some("work:collector_heartbeat")),
            (ActionType::KeepAlive, None),
            (ActionType::ReleaseMemory, Some("cache:prefetch")),
            (ActionType::ReleaseMemory, None),
        ] {
            let action = SuggestedAction {
                action_type: action_type.clone(),
                target: target.map(|s| s.to_string()),
                urgency: ActionUrgency::Immediate,
            };
            assert!(
                matches!(classify(&action), Route::Forward(_)),
                "{:?} with target {:?} must forward",
                action.action_type,
                action.target,
            );
        }
    }

    /// 启动一个一次性 TCP 服务端：读到 EOF 后回送 `response`。
    fn spawn_responder(response: &'static [u8]) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = Vec::new();
            let mut chunk = [0u8; 1024];
            loop {
                match stream.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => buf.extend_from_slice(&chunk[..n]),
                    Err(_) => break,
                }
            }
            stream.write_all(response).unwrap();
            stream.flush().unwrap();
            stream.shutdown(Shutdown::Write).ok();
        });
        port
    }

    #[test]
    fn send_request_returns_response_bytes() {
        let port = spawn_responder(br#"{"status":"ok","summary":"done","latency_us":7}"#);
        let raw = send_request("127.0.0.1", port, b"req").unwrap();
        let parsed: BridgeExecuteResponse = serde_json::from_slice(&raw).unwrap();
        assert_eq!(parsed.status, BridgeStatus::Ok);
        assert_eq!(parsed.summary.as_deref(), Some("done"));
        assert_eq!(parsed.latency_us, Some(7));
    }

    #[test]
    fn send_request_fails_closed_on_refused_connection() {
        // Nothing is listening on this port → connect fails → Err (never silent success).
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener); // free the port so connect is refused
        let err = send_request("127.0.0.1", port, b"req").unwrap_err();
        assert!(err.contains("connect Android bridge"), "got: {err}");
    }

    #[test]
    fn send_request_fails_when_peer_closes_without_response() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            // Drain the request (avoid an RST from unread data), then close
            // without replying: the client sees a clean EOF with empty buffer.
            let mut chunk = [0u8; 1024];
            loop {
                match stream.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(_) => {},
                    Err(_) => break,
                }
            }
            drop(stream);
        });
        let err = send_request("127.0.0.1", port, b"req").unwrap_err();
        assert!(err.contains("closed without response"), "got: {err}");
    }
}
