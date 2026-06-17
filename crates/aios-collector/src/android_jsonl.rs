//! Android collector JSONL ingress.
//!
//! The Android app writes one `CollectorEvent` JSON object per line. When a
//! row contains a Rust-compatible `rawEvent` field, this module wraps it in a
//! `CollectorEnvelope` so the daemon can feed it into the normal
//! `RustCollectorIngress -> PrivacyAirGap -> WindowAggregator` pipeline.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use aios_spec::{CollectorEnvelope, RawEvent, SourceTier};
use serde_json::Value;
use thiserror::Error;

const SCHEMA_VERSION: &str = "dipecs.collector.v1";
const DEFAULT_SOURCE: &str = "apps.android-collector";

#[derive(Debug, Clone)]
pub struct AndroidJsonlIngress {
    source_tier: SourceTier,
}

impl AndroidJsonlIngress {
    pub fn new() -> Self {
        Self {
            source_tier: SourceTier::PublicApi,
        }
    }

    pub fn parse_line(&self, line: &str) -> Result<Option<CollectorEnvelope>, AndroidJsonlError> {
        if line.trim().is_empty() {
            return Ok(None);
        }

        let value: Value = serde_json::from_str(line)?;
        let raw_event_value = value.get("rawEvent").cloned().unwrap_or(Value::Null);
        if raw_event_value.is_null() {
            return Ok(None);
        }

        let raw_event: RawEvent = serde_json::from_value(raw_event_value)?;
        Ok(Some(CollectorEnvelope {
            schema_version: SCHEMA_VERSION.to_string(),
            source: value
                .get("source")
                .and_then(Value::as_str)
                .unwrap_or(DEFAULT_SOURCE)
                .to_string(),
            source_tier: self.source_tier,
            device_trace_id: value
                .get("eventId")
                .and_then(Value::as_str)
                .map(String::from),
            captured_at_ms: value
                .get("timestampMs")
                .and_then(Value::as_i64)
                .or_else(|| event_timestamp_ms(&raw_event))
                .unwrap_or(0),
            received_at_ms: None,
            raw_event,
        }))
    }
}

impl Default for AndroidJsonlIngress {
    fn default() -> Self {
        Self::new()
    }
}

/// Polls an append-only Android `actions.jsonl` file.
///
/// The tailer keeps byte offset state between polls. If the file is truncated
/// or rotated, the offset is reset and the next poll starts from the beginning
/// of the new file.
#[derive(Debug)]
pub struct AndroidJsonlTailer {
    path: PathBuf,
    offset: u64,
    partial_line: String,
    ingress: AndroidJsonlIngress,
}

impl AndroidJsonlTailer {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            offset: 0,
            partial_line: String::new(),
            ingress: AndroidJsonlIngress::new(),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn poll(&mut self) -> Result<Vec<CollectorEnvelope>, AndroidJsonlError> {
        let mut file = match File::open(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(AndroidJsonlError::Io(error)),
        };

        let len = file.metadata()?.len();
        if len < self.offset {
            self.offset = 0;
            self.partial_line.clear();
        }

        file.seek(SeekFrom::Start(self.offset))?;
        let mut chunk = String::new();
        let bytes_read = file.read_to_string(&mut chunk)? as u64;
        self.offset += bytes_read;

        if chunk.is_empty() {
            return Ok(Vec::new());
        }

        let mut pending = String::new();
        std::mem::swap(&mut pending, &mut self.partial_line);
        pending.push_str(&chunk);

        let mut lines: Vec<&str> = pending.split('\n').collect();
        if !pending.ends_with('\n') {
            if let Some(last) = lines.pop() {
                self.partial_line = last.to_string();
            }
        }

        let mut envelopes = Vec::new();
        for line in lines {
            let trimmed = line.trim_end_matches('\r');
            if let Some(envelope) = self.ingress.parse_line(trimmed)? {
                envelopes.push(envelope);
            }
        }
        Ok(envelopes)
    }
}

#[derive(Debug, Error)]
pub enum AndroidJsonlError {
    #[error("read Android JSONL trace failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("parse Android CollectorEvent JSON failed: {0}")]
    Json(#[from] serde_json::Error),
}

fn event_timestamp_ms(raw_event: &RawEvent) -> Option<i64> {
    Some(match raw_event {
        RawEvent::AppTransition(event) => event.timestamp_ms,
        RawEvent::BinderTransaction(event) => event.timestamp_ms,
        RawEvent::ProcStateChange(event) => event.timestamp_ms,
        RawEvent::FileSystemAccess(event) => event.timestamp_ms,
        RawEvent::NotificationPosted(event) => event.timestamp_ms,
        RawEvent::NotificationInteraction(event) => event.timestamp_ms,
        RawEvent::ScreenState(event) => event.timestamp_ms,
        RawEvent::SystemState(event) => event.timestamp_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::{AndroidJsonlIngress, AndroidJsonlTailer};
    use aios_spec::{AppTransition, RawEvent, SourceTier};
    use std::fs::OpenOptions;
    use std::io::Write;
    use std::time::{SystemTime, UNIX_EPOCH};

    const APP_TRANSITION_LINE: &str = r#"{"eventId":"evt-1","timestampMs":1000,"source":"UsageCollector","eventType":"app_transition","rawEvent":{"AppTransition":{"timestamp_ms":1000,"package_name":"com.android.chrome","activity_class":"MainActivity","transition":"Foreground"}},"rawPayload":{}}"#;

    #[test]
    fn parse_line_wraps_raw_event_in_collector_envelope() {
        let envelope = AndroidJsonlIngress::new()
            .parse_line(APP_TRANSITION_LINE)
            .unwrap()
            .unwrap();

        assert_eq!(envelope.schema_version, "dipecs.collector.v1");
        assert_eq!(envelope.source, "UsageCollector");
        assert_eq!(envelope.source_tier, SourceTier::PublicApi);
        assert_eq!(envelope.device_trace_id.as_deref(), Some("evt-1"));
        assert_eq!(envelope.captured_at_ms, 1000);
        match envelope.raw_event {
            RawEvent::AppTransition(event) => {
                assert_eq!(event.package_name, "com.android.chrome");
                assert_eq!(event.transition, AppTransition::Foreground);
            },
            other => panic!("unexpected raw event: {other:?}"),
        }
    }

    #[test]
    fn parse_line_skips_rows_without_raw_event() {
        let line = r#"{"eventId":"evt-2","timestampMs":1000,"source":"AccessibilityCollectorService","rawEvent":null}"#;
        let parsed = AndroidJsonlIngress::new().parse_line(line).unwrap();
        assert!(parsed.is_none());
    }

    #[test]
    fn tailer_reads_only_new_complete_lines() {
        let path = temp_trace_path();
        {
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .unwrap();
            writeln!(file, "{APP_TRANSITION_LINE}").unwrap();
        }

        let mut tailer = AndroidJsonlTailer::new(&path);
        assert_eq!(tailer.poll().unwrap().len(), 1);
        assert!(tailer.poll().unwrap().is_empty());

        {
            let mut file = OpenOptions::new().append(true).open(&path).unwrap();
            write!(file, "{APP_TRANSITION_LINE}").unwrap();
        }
        assert!(tailer.poll().unwrap().is_empty());

        {
            let mut file = OpenOptions::new().append(true).open(&path).unwrap();
            writeln!(file).unwrap();
        }
        assert_eq!(tailer.poll().unwrap().len(), 1);

        let _ = std::fs::remove_file(path);
    }

    fn temp_trace_path() -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("dipecs-android-jsonl-{unique}.jsonl"))
    }
}
