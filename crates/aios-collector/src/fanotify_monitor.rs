//! Fanotify monitor interface for privileged filesystem access events.
//!
//! The public API is intentionally usable on every development platform. Real
//! fanotify attachment is only available to a privileged Linux daemon; when the
//! current process cannot attach, the monitor reports that status and returns an
//! empty event stream instead of pretending to collect file activity.

use std::path::{Path, PathBuf};

use aios_spec::{FsAccessEvent, FsAccessType, RawEvent};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FanotifyConfig {
    pub roots: Vec<PathBuf>,
    pub max_events_per_poll: usize,
}

impl Default for FanotifyConfig {
    fn default() -> Self {
        Self {
            roots: vec![PathBuf::from("/")],
            max_events_per_poll: 256,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FanotifyStatus {
    Uninitialized,
    UnsupportedPlatform,
    PermissionRequired,
    Ready,
}

pub struct FanotifyMonitor {
    config: FanotifyConfig,
    status: FanotifyStatus,
}

impl FanotifyMonitor {
    pub fn new(config: FanotifyConfig) -> Self {
        Self {
            config,
            status: FanotifyStatus::Uninitialized,
        }
    }

    pub fn status(&self) -> FanotifyStatus {
        self.status
    }

    pub fn roots(&self) -> &[PathBuf] {
        &self.config.roots
    }

    pub fn try_init(&mut self) -> FanotifyStatus {
        self.status = detect_status();
        self.status
    }

    pub fn poll(&self) -> Vec<RawEvent> {
        if self.status != FanotifyStatus::Ready {
            return Vec::new();
        }

        // A real implementation should read from the fanotify fd, cap the
        // batch at `max_events_per_poll`, and convert each record with
        // `event_from_observation` below. Keeping this empty until an fd-backed
        // implementation exists preserves deterministic local tests.
        Vec::new()
    }

    pub fn event_from_observation(
        timestamp_ms: i64,
        pid: u32,
        uid: u32,
        file_path: impl AsRef<Path>,
        access_type: FsAccessType,
        bytes_transferred: Option<u64>,
    ) -> RawEvent {
        RawEvent::FileSystemAccess(FsAccessEvent {
            timestamp_ms,
            pid,
            uid,
            file_path: file_path.as_ref().to_string_lossy().into_owned(),
            access_type,
            bytes_transferred,
        })
    }
}

impl Default for FanotifyMonitor {
    fn default() -> Self {
        Self::new(FanotifyConfig::default())
    }
}

fn detect_status() -> FanotifyStatus {
    if !cfg!(target_os = "linux") {
        return FanotifyStatus::UnsupportedPlatform;
    }

    // On Linux, fanotify requires a privileged daemon and a real fd-backed
    // implementation. Until that implementation is linked in, expose the
    // deployment requirement explicitly.
    FanotifyStatus::PermissionRequired
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_or_unprivileged_monitor_returns_no_events() {
        let mut monitor = FanotifyMonitor::default();
        let status = monitor.try_init();

        assert!(matches!(
            status,
            FanotifyStatus::UnsupportedPlatform | FanotifyStatus::PermissionRequired
        ));
        assert!(monitor.poll().is_empty());
    }

    #[test]
    fn observation_conversion_preserves_raw_path_until_airgap() {
        let raw = FanotifyMonitor::event_from_observation(
            42,
            100,
            200,
            "/tmp/private/report.docx",
            FsAccessType::OpenRead,
            Some(4096),
        );

        match raw {
            RawEvent::FileSystemAccess(event) => {
                assert_eq!(event.timestamp_ms, 42);
                assert_eq!(event.pid, 100);
                assert_eq!(event.uid, 200);
                assert_eq!(event.file_path, "/tmp/private/report.docx");
                assert!(matches!(event.access_type, FsAccessType::OpenRead));
                assert_eq!(event.bytes_transferred, Some(4096));
            },
            other => panic!("expected FileSystemAccess, got {other:?}"),
        }
    }
}
