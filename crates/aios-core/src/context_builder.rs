//! 上下文构建器 — 将脱敏事件按时间窗口聚合
//!
//! 将 `SanitizedEvent` 序列聚合成 `StructuredContext`,
//! 这是发送给 Cloud LLM 前的最后一步。

use std::collections::HashMap;

use aios_spec::{
    AppTransition, ContextSummary, ExtensionCategory, SanitizedEvent, SanitizedEventType,
    SemanticHint, SourceTier, StructuredContext, SystemStatusSnapshot,
};
use uuid::Uuid;

/// 窗口聚合器
///
/// 按时间窗口收集 SanitizedEvent, 在窗口关闭时构建 StructuredContext。
#[derive(Debug)]
pub struct WindowAggregator {
    /// 当前窗口的事件缓冲区
    buffer: Vec<SanitizedEvent>,
    /// 窗口时长 (秒)
    window_secs: u64,
    /// 窗口起始时间 (epoch ms)
    window_start_ms: i64,
}

impl WindowAggregator {
    /// 创建新的窗口聚合器
    ///
    /// `window_secs` 为窗口长度, 默认 10 秒。
    /// `now_ms` 为当前 epoch 毫秒时间戳。
    pub fn new(window_secs: u64, now_ms: i64) -> Self {
        Self {
            buffer: Vec::new(),
            window_secs,
            window_start_ms: now_ms,
        }
    }

    /// 向当前窗口追加一个脱敏事件
    pub fn push(&mut self, event: SanitizedEvent) {
        self.buffer.push(event);
    }

    /// 返回当前窗口内的已收集事件数
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    /// 返回 true 表示窗口为空
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// 窗口是否已到期
    pub fn is_expired(&self, now_ms: i64) -> bool {
        let elapsed_ms = now_ms.saturating_sub(self.window_start_ms);
        elapsed_ms >= (self.window_secs * 1000) as i64
    }

    /// 关闭当前窗口, 构建 StructuredContext, 并重置缓冲区。
    ///
    /// `now_ms` 为窗口结束时间 (epoch ms)。
    /// 返回 Some(StructuredContext) 如果窗口非空, 否则返回 None。
    pub fn close(&mut self, now_ms: i64) -> Option<StructuredContext> {
        if self.buffer.is_empty() {
            self.window_start_ms = now_ms;
            return None;
        }

        let events = std::mem::take(&mut self.buffer);
        let summary = build_summary(&events);
        let context = StructuredContext {
            window_id: new_id(),
            window_start_ms: self.window_start_ms,
            window_end_ms: now_ms,
            duration_secs: ((now_ms - self.window_start_ms).max(0) / 1000) as u32,
            events,
            summary,
        };

        self.window_start_ms = now_ms;

        Some(context)
    }
}

/// 从事件序列构建 ContextSummary
fn build_summary(events: &[SanitizedEvent]) -> ContextSummary {
    let mut foreground_apps: Vec<String> = Vec::new();
    let mut notified_apps: Vec<String> = Vec::new();
    let mut all_semantic_hints: Vec<SemanticHint> = Vec::new();
    let mut file_activity_counts: HashMap<ExtensionCategory, u32> = HashMap::new();
    let mut latest_system_status: Option<SystemStatusSnapshot> = None;
    let mut source_tier = SourceTier::PublicApi;

    for event in events {
        if event.source_tier == SourceTier::Daemon {
            source_tier = SourceTier::Daemon;
        }

        match &event.event_type {
            SanitizedEventType::AppTransition {
                package_name,
                transition: AppTransition::Foreground,
                ..
            } if !foreground_apps.contains(package_name) => {
                foreground_apps.push(package_name.clone());
            },
            SanitizedEventType::ProcessResource {
                package_name: Some(pkg),
                ..
            } if !foreground_apps.contains(pkg) => {
                foreground_apps.push(pkg.clone());
            },
            SanitizedEventType::Notification {
                source_package,
                semantic_hints,
                ..
            } => {
                if !notified_apps.contains(source_package) {
                    notified_apps.push(source_package.clone());
                }
                for hint in semantic_hints {
                    if !all_semantic_hints.contains(hint) {
                        all_semantic_hints.push(hint.clone());
                    }
                }
            },
            SanitizedEventType::FileActivity {
                extension_category, ..
            } => {
                *file_activity_counts
                    .entry(extension_category.clone())
                    .or_insert(0) += 1;
            },
            SanitizedEventType::SystemStatus {
                battery_pct,
                is_charging,
                network,
                ringer_mode,
                location_type,
                headphone_connected,
            } => {
                latest_system_status = Some(SystemStatusSnapshot {
                    battery_pct: *battery_pct,
                    is_charging: *is_charging,
                    network: network.clone(),
                    ringer_mode: ringer_mode.clone(),
                    location_type: location_type.clone(),
                    headphone_connected: *headphone_connected,
                });
            },
            SanitizedEventType::InterAppInteraction {
                source_package: Some(pkg),
                ..
            } if !foreground_apps.contains(pkg) => {
                foreground_apps.push(pkg.clone());
            },
            _ => {},
        }
    }

    let file_activity: Vec<(ExtensionCategory, u32)> = file_activity_counts.into_iter().collect();

    ContextSummary {
        foreground_apps,
        notified_apps,
        all_semantic_hints,
        file_activity,
        latest_system_status,
        source_tier,
    }
}

fn new_id() -> String {
    Uuid::new_v4().to_string()
}
