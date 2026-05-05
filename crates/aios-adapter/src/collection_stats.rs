//! 采集观测统计
//!
//! 只记录原始事件类型计数, 不读取或保留事件内容。

use aios_spec::RawEvent;

/// 原始事件类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawEventKind {
    /// UsageStatsManager 应用前后台切换。
    AppTransition,
    /// Binder 事务。
    BinderTransaction,
    /// /proc 进程状态变化。
    ProcStateChange,
    /// 文件系统访问。
    FileSystemAccess,
    /// 通知到达。
    NotificationPosted,
    /// 通知交互。
    NotificationInteraction,
    /// 屏幕状态。
    ScreenState,
    /// 系统状态。
    SystemState,
}

/// 窗口内的原始采集统计。
#[derive(Debug, Clone, Default)]
pub struct RawEventStats {
    app_transition: u64,
    binder_transaction: u64,
    proc_state_change: u64,
    file_system_access: u64,
    notification_posted: u64,
    notification_interaction: u64,
    screen_state: u64,
    system_state: u64,
}

impl RawEventStats {
    /// 记录一个原始事件。
    pub fn record(&mut self, event: &RawEvent) {
        match event {
            RawEvent::AppTransition(_) => self.app_transition += 1,
            RawEvent::BinderTransaction(_) => self.binder_transaction += 1,
            RawEvent::ProcStateChange(_) => self.proc_state_change += 1,
            RawEvent::FileSystemAccess(_) => self.file_system_access += 1,
            RawEvent::NotificationPosted(_) => self.notification_posted += 1,
            RawEvent::NotificationInteraction(_) => self.notification_interaction += 1,
            RawEvent::ScreenState(_) => self.screen_state += 1,
            RawEvent::SystemState(_) => self.system_state += 1,
        }
    }

    /// 返回指定事件类型的计数。
    pub fn count(&self, kind: RawEventKind) -> u64 {
        match kind {
            RawEventKind::AppTransition => self.app_transition,
            RawEventKind::BinderTransaction => self.binder_transaction,
            RawEventKind::ProcStateChange => self.proc_state_change,
            RawEventKind::FileSystemAccess => self.file_system_access,
            RawEventKind::NotificationPosted => self.notification_posted,
            RawEventKind::NotificationInteraction => self.notification_interaction,
            RawEventKind::ScreenState => self.screen_state,
            RawEventKind::SystemState => self.system_state,
        }
    }

    /// 返回窗口内原始事件总数。
    pub fn total(&self) -> u64 {
        self.summary_fields()
            .into_iter()
            .map(|(_, count)| count)
            .sum()
    }

    /// 返回适合日志输出的稳定字段列表。
    pub fn summary_fields(&self) -> Vec<(&'static str, u64)> {
        vec![
            ("app_transition", self.app_transition),
            ("binder_transaction", self.binder_transaction),
            ("proc_state_change", self.proc_state_change),
            ("file_system_access", self.file_system_access),
            ("notification_posted", self.notification_posted),
            ("notification_interaction", self.notification_interaction),
            ("screen_state", self.screen_state),
            ("system_state", self.system_state),
        ]
    }

    /// 返回适合单行日志输出的非零统计。
    pub fn summary_line(&self) -> String {
        let parts: Vec<String> = self
            .summary_fields()
            .into_iter()
            .filter(|(_, count)| *count > 0)
            .map(|(kind, count)| format!("{kind}={count}"))
            .collect();

        if parts.is_empty() {
            "none".to_string()
        } else {
            parts.join(" ")
        }
    }
}
