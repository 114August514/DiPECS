//! Binder/eBPF probe interface for Android IPC observation.
//!
//! The probe is safe to construct on every development platform. A real Binder
//! eBPF attachment requires a privileged Android/Linux daemon with kernel BPF
//! support and Binder tracepoints. When those requirements are not met, the
//! probe reports a clear status and returns an empty event stream.

use std::path::Path;

use aios_spec::BinderTxEvent;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinderProbeStatus {
    Uninitialized,
    UnsupportedPlatform,
    TracepointUnavailable,
    PermissionRequired,
    Ready,
}

/// Binder probe subscribing to Binder transaction events.
pub struct BinderProbe {
    status: BinderProbeStatus,
}

/// Binder transaction decoded from an eBPF tracepoint record.
#[derive(Debug, Clone)]
pub struct BinderTransaction {
    pub timestamp_ms: i64,
    pub source_pid: u32,
    pub source_uid: u32,
    /// Target Binder service name, for example "notification" or "activity".
    pub target_service: String,
    /// Target method inferred from Binder transaction code.
    pub target_method: String,
    /// Whether the transaction is oneway.
    pub is_oneway: bool,
    /// Parcel payload size in bytes. Payload content is never stored.
    pub payload_size: u32,
}

impl BinderProbe {
    pub fn new() -> Self {
        Self {
            status: BinderProbeStatus::Uninitialized,
        }
    }

    pub fn status(&self) -> BinderProbeStatus {
        self.status
    }

    /// Attempts to initialize Binder/eBPF observation.
    ///
    /// `Ok(true)` means the probe can be polled. `Ok(false)` means the current
    /// environment cannot support privileged Binder tracing and the daemon
    /// should degrade to public/API and `/proc` inputs.
    pub fn try_init(&mut self) -> Result<bool, ProbeError> {
        self.status = detect_status();
        match self.status {
            BinderProbeStatus::Ready => {
                tracing::info!("Binder tracepoint detected, probe initialized");
                Ok(true)
            },
            BinderProbeStatus::PermissionRequired => {
                tracing::warn!("Binder tracepoint exists but eBPF attachment requires privilege");
                Ok(false)
            },
            BinderProbeStatus::UnsupportedPlatform | BinderProbeStatus::TracepointUnavailable => {
                tracing::warn!("Binder tracepoint not available; probe will return no events");
                Ok(false)
            },
            BinderProbeStatus::Uninitialized => Ok(false),
        }
    }

    /// Polls Binder events observed since the previous call.
    pub fn poll(&self) -> Vec<BinderTransaction> {
        if self.status != BinderProbeStatus::Ready {
            return Vec::new();
        }

        // A real implementation should load a BPF program, attach it to
        // tracepoint/binder/binder_transaction, and drain a ring/perf buffer.
        // Until that fd-backed path exists, keep the stream empty and explicit.
        Vec::new()
    }
}

impl Default for BinderProbe {
    fn default() -> Self {
        Self::new()
    }
}

impl BinderTransaction {
    pub fn to_event(&self) -> BinderTxEvent {
        BinderTxEvent {
            timestamp_ms: self.timestamp_ms,
            source_pid: self.source_pid,
            source_uid: self.source_uid,
            target_service: self.target_service.clone(),
            target_method: self.target_method.clone(),
            is_oneway: self.is_oneway,
            payload_size: self.payload_size,
        }
    }
}

fn detect_status() -> BinderProbeStatus {
    if !cfg!(target_os = "linux") {
        return BinderProbeStatus::UnsupportedPlatform;
    }

    let binder_trace =
        Path::new("/sys/kernel/debug/tracing/events/binder/binder_transaction/enable");
    if !binder_trace.exists() {
        return BinderProbeStatus::TracepointUnavailable;
    }

    // The tracepoint exists, but this crate does not yet link an fd-backed BPF
    // loader. Report the remaining deployment requirement explicitly.
    BinderProbeStatus::PermissionRequired
}

#[derive(Debug, thiserror::Error)]
pub enum ProbeError {
    #[error("eBPF not supported on this kernel")]
    EbpfNotSupported,
    #[error("insufficient permissions for eBPF (need root or system daemon)")]
    PermissionDenied,
    #[error("BPF program load failed: {0}")]
    LoadError(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_probe_returns_no_events() {
        let mut probe = BinderProbe::new();
        let initialized = probe.try_init().expect("status detection should not fail");

        assert!(!initialized);
        assert!(matches!(
            probe.status(),
            BinderProbeStatus::UnsupportedPlatform
                | BinderProbeStatus::TracepointUnavailable
                | BinderProbeStatus::PermissionRequired
        ));
        assert!(probe.poll().is_empty());
    }

    #[test]
    fn binder_transaction_converts_to_raw_event_payload() {
        let tx = BinderTransaction {
            timestamp_ms: 10,
            source_pid: 11,
            source_uid: 12,
            target_service: "activity".into(),
            target_method: "startActivity".into(),
            is_oneway: false,
            payload_size: 128,
        };

        let event = tx.to_event();
        assert_eq!(event.timestamp_ms, 10);
        assert_eq!(event.source_pid, 11);
        assert_eq!(event.source_uid, 12);
        assert_eq!(event.target_service, "activity");
        assert_eq!(event.target_method, "startActivity");
        assert!(!event.is_oneway);
        assert_eq!(event.payload_size, 128);
    }
}
