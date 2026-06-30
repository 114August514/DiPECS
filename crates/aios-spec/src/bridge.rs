//! Android localhost bridge 线协议（IPC schema，归属 SSOT）。
//!
//! `execute` 请求把一个已封存的 `AuthorizedAction`（在 `aios-action` 侧序列化为
//! canonical 字符串）连同 freshness window，以及覆盖 freshness window 与 action
//! 字节的 HMAC-SHA256 认证标签发往设备侧 bridge；设备执行后回送结构化的
//! [`BridgeExecuteResponse`]。
//!
//! 设计要点：
//! - 认证标签 (`auth.hmac_sha256`) 放在 envelope 上，而非塞进 action JSON 内部，
//!   使每次授权与一个**具体的 action 字节序列和有效期**绑定 —— 捕获到的旧标签
//!   无法重放到另一个 action 或过期窗口（替换静态 bearer token 的关键改进）。
//! - `action` 以**字符串**形式承载（即 `AuthorizedAction` 序列化后的逐字节内容），
//!   HMAC 输入使用 length-prefixed action 字节和 freshness window。两侧都对
//!   "收发的同一段字节"做 HMAC，规避跨语言 JSON key 排序导致的
//!   canonicalization 漂移。
//!
//! 注意：本 crate 零内部依赖，这里只定义协议数据；HMAC 计算与 TCP 收发在
//! `aios-action` 侧实现。设备（Kotlin）侧的 responder 是 Tier 2 工作，须遵循本契约。

use serde::{Deserialize, Serialize};

/// `execute` 请求的 `message_type` 取值。与健康检查的 `"ping"` 区分。
pub const BRIDGE_MESSAGE_TYPE_EXECUTE: &str = "execute";

/// 设备侧执行请求 envelope。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeExecuteRequest {
    /// 固定为 [`BRIDGE_MESSAGE_TYPE_EXECUTE`]。
    pub message_type: String,
    /// 请求签发时间（epoch milliseconds）。
    pub issued_at_ms: i64,
    /// 请求过期时间（epoch milliseconds）。
    pub expires_at_ms: i64,
    /// 认证标签，绑定到 `action` 字节。
    pub auth: BridgeAuth,
    /// canonical 序列化后的 `AuthorizedAction`。
    pub action: String,
}

/// envelope 级认证标签。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeAuth {
    /// 对 freshness window 和 [`BridgeExecuteRequest::action`] 字节的 HMAC-SHA256，小写 hex。
    pub hmac_sha256: String,
}

/// 设备侧执行状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BridgeStatus {
    /// 设备已真实执行成功。
    Ok,
    /// 设备侧二次治理拒绝（token/whitelist/PII 复检未通过）。
    Rejected,
    /// 设备侧执行过程中出错。
    Error,
}

/// 设备侧执行结果。`summary`/`latency_us` 仅在 `status == Ok` 时有意义。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeExecuteResponse {
    pub status: BridgeStatus,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub latency_us: Option<u64>,
    #[serde(default)]
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execute_request_roundtrips() {
        let request = BridgeExecuteRequest {
            message_type: BRIDGE_MESSAGE_TYPE_EXECUTE.into(),
            issued_at_ms: 1000,
            expires_at_ms: 2000,
            auth: BridgeAuth {
                hmac_sha256: "deadbeef".into(),
            },
            action: r#"{"intent_id":"x"}"#.into(),
        };
        let json = serde_json::to_string(&request).unwrap();
        let parsed: BridgeExecuteRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.message_type, BRIDGE_MESSAGE_TYPE_EXECUTE);
        assert_eq!(parsed.issued_at_ms, 1000);
        assert_eq!(parsed.expires_at_ms, 2000);
        assert_eq!(parsed.auth.hmac_sha256, "deadbeef");
        assert_eq!(parsed.action, request.action);
    }

    #[test]
    fn response_status_serializes_snake_case() {
        let response = BridgeExecuteResponse {
            status: BridgeStatus::Ok,
            summary: Some("prefetched".into()),
            latency_us: Some(1234),
            error: None,
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"status\":\"ok\""), "got {json}");
        let parsed: BridgeExecuteResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.status, BridgeStatus::Ok);
        assert_eq!(parsed.latency_us, Some(1234));
    }

    #[test]
    fn response_tolerates_missing_optional_fields() {
        let parsed: BridgeExecuteResponse =
            serde_json::from_str(r#"{"status":"rejected"}"#).unwrap();
        assert_eq!(parsed.status, BridgeStatus::Rejected);
        assert_eq!(parsed.summary, None);
        assert_eq!(parsed.error, None);
    }
}
