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
const FIELD_RAW_EVENT: &str = "rawEvent";
const FIELD_SOURCE: &str = "source";
const FIELD_EVENT_ID: &str = "eventId";
const FIELD_TIMESTAMP_MS: &str = "timestampMs";

/// Parsed outcome for one Android JSONL row.
///
/// `parse_line` keeps the historical `Option<CollectorEnvelope>` API for
/// callers that only need ingestion. This shape is intentionally more verbose
/// for code that wants to keep counters or diagnostics without re-parsing the
/// original JSON row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AndroidJsonlRecord {
    EmptyLine,
    NoRawEvent,
    Envelope(Box<CollectorEnvelope>),
}

impl AndroidJsonlRecord {
    pub fn into_envelope(self) -> Option<CollectorEnvelope> {
        match self {
            Self::Envelope(envelope) => Some(*envelope),
            Self::EmptyLine | Self::NoRawEvent => None,
        }
    }
}

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
        let record = self.parse_record(line)?;
        Ok(record.into_envelope())
    }

    pub fn parse_record(&self, line: &str) -> Result<AndroidJsonlRecord, AndroidJsonlError> {
        if line.trim().is_empty() {
            return Ok(AndroidJsonlRecord::EmptyLine);
        }

        let value: Value = serde_json::from_str(line)?;
        let Some(raw_event_value) = raw_event_json(&value) else {
            return Ok(AndroidJsonlRecord::NoRawEvent);
        };

        let raw_event = parse_raw_event(raw_event_value)?;
        let source = collector_source(&value);
        let device_trace_id = collector_event_id(&value);
        let captured_at_ms = collector_timestamp_ms(&value, &raw_event);

        let envelope = CollectorEnvelope {
            schema_version: SCHEMA_VERSION.to_string(),
            source,
            source_tier: self.source_tier,
            device_trace_id,
            captured_at_ms,
            received_at_ms: None,
            raw_event,
        };

        Ok(AndroidJsonlRecord::Envelope(Box::new(envelope)))
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
        let Some(chunk) = self.read_new_chunk()? else {
            return Ok(Vec::new());
        };

        let complete_lines = self.take_complete_lines(chunk);
        let envelopes = self.parse_complete_lines(complete_lines)?;

        Ok(envelopes)
    }

    fn read_new_chunk(&mut self) -> Result<Option<String>, AndroidJsonlError> {
        let mut file = match File::open(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(AndroidJsonlError::Io(error)),
        };

        let file_len = file.metadata()?.len();
        if self.offset_is_past_end(file_len) {
            self.reset_for_recreated_file();
        }

        let old_offset = self.offset;
        let mut chunk = self.read_chunk_at_offset(&mut file, old_offset)?;

        if self.should_rewind_after_chunk_read(old_offset, &chunk) {
            self.reset_for_recreated_file();
            chunk = self.read_chunk_at_offset(&mut file, 0)?;
        }

        if chunk.is_empty() {
            Ok(None)
        } else {
            Ok(Some(chunk))
        }
    }

    fn read_chunk_at_offset(
        &mut self,
        file: &mut File,
        offset: u64,
    ) -> Result<String, AndroidJsonlError> {
        file.seek(SeekFrom::Start(offset))?;

        let mut chunk = String::new();
        let bytes_read = file.read_to_string(&mut chunk)? as u64;
        self.offset = offset + bytes_read;

        Ok(chunk)
    }

    fn offset_is_past_end(&self, file_len: u64) -> bool {
        file_len < self.offset
    }

    fn reset_for_recreated_file(&mut self) {
        self.offset = 0;
        self.partial_line.clear();
    }

    fn should_rewind_after_chunk_read(&self, old_offset: u64, chunk: &str) -> bool {
        if old_offset == 0 || !self.partial_line.is_empty() {
            return false;
        }

        let Some(first_char) = chunk.chars().find(|c| !c.is_whitespace()) else {
            return false;
        };

        first_char != '{'
    }

    fn take_complete_lines(&mut self, chunk: String) -> Vec<String> {
        let mut pending = self.pending_with_new_chunk(chunk);
        let has_partial_tail = !pending.ends_with('\n');
        let mut complete_lines = Vec::new();

        if has_partial_tail {
            let partial_start = pending.rfind('\n').map(|idx| idx + 1).unwrap_or(0);
            self.partial_line = pending.split_off(partial_start);
        }

        for line in pending.split('\n') {
            if line.is_empty() {
                continue;
            }
            complete_lines.push(line.trim_end_matches('\r').to_string());
        }

        complete_lines
    }

    fn pending_with_new_chunk(&mut self, chunk: String) -> String {
        if self.partial_line.is_empty() {
            return chunk;
        }

        let mut pending = String::new();
        std::mem::swap(&mut pending, &mut self.partial_line);
        pending.push_str(&chunk);
        pending
    }

    fn parse_complete_lines(
        &self,
        complete_lines: Vec<String>,
    ) -> Result<Vec<CollectorEnvelope>, AndroidJsonlError> {
        let mut envelopes = Vec::new();

        for line in complete_lines {
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

fn raw_event_json(value: &Value) -> Option<Value> {
    match value.get(FIELD_RAW_EVENT) {
        Some(raw_event) if !raw_event.is_null() => Some(raw_event.clone()),
        Some(_) | None => None,
    }
}

fn parse_raw_event(value: Value) -> Result<RawEvent, AndroidJsonlError> {
    let raw_event = serde_json::from_value(value)?;
    Ok(raw_event)
}

fn collector_source(value: &Value) -> String {
    value
        .get(FIELD_SOURCE)
        .and_then(Value::as_str)
        .filter(|source| !source.trim().is_empty())
        .unwrap_or(DEFAULT_SOURCE)
        .to_string()
}

fn collector_event_id(value: &Value) -> Option<String> {
    value
        .get(FIELD_EVENT_ID)
        .and_then(Value::as_str)
        .filter(|event_id| !event_id.trim().is_empty())
        .map(String::from)
}

fn collector_timestamp_ms(value: &Value, raw_event: &RawEvent) -> i64 {
    value
        .get(FIELD_TIMESTAMP_MS)
        .and_then(Value::as_i64)
        .or_else(|| event_timestamp_ms(raw_event))
        .unwrap_or(0)
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
    use super::{AndroidJsonlIngress, AndroidJsonlRecord, AndroidJsonlTailer, DEFAULT_SOURCE};
    use aios_spec::{AppTransition, NetworkType, RawEvent, SourceTier};
    use std::fs::OpenOptions;
    use std::io::Write;
    use std::time::{SystemTime, UNIX_EPOCH};

    const APP_TRANSITION_LINE: &str = r#"{"eventId":"evt-1","timestampMs":1000,"source":"UsageCollector","eventType":"app_transition","rawEvent":{"AppTransition":{"timestamp_ms":1000,"package_name":"com.android.chrome","activity_class":"MainActivity","transition":"Foreground"}},"rawPayload":{}}"#;
    const NOTIFICATION_POSTED_LINE: &str = r#"{"eventId":"evt-2","timestampMs":2000,"source":"NotificationCollectorService","eventType":"notification_posted","rawEvent":{"NotificationPosted":{"timestamp_ms":2000,"package_name":"com.chat","category":"msg","channel_id":"messages","raw_title":"Alice","raw_text":"sent a file","is_ongoing":false,"group_key":"conversation","has_picture":false}},"rawPayload":{}}"#;
    const SYSTEM_STATE_LINE: &str = r#"{"eventId":"evt-3","timestampMs":3000,"source":"CollectorForegroundService","eventType":"system_state","rawEvent":{"SystemState":{"timestamp_ms":3000,"battery_pct":88,"is_charging":true,"network":"Wifi","ringer_mode":"Normal","location_type":"Unknown","headphone_connected":false,"bluetooth_connected":false}},"rawPayload":{}}"#;

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
    fn parse_record_distinguishes_empty_and_no_raw_event_rows() {
        let ingress = AndroidJsonlIngress::new();

        let empty = ingress.parse_record("  \r\n").unwrap();
        assert_eq!(empty, AndroidJsonlRecord::EmptyLine);

        let no_raw_event = ingress
            .parse_record(
                r#"{"eventId":"evt-screen","source":"AccessibilityCollectorService","rawEvent":null}"#,
            )
            .unwrap();
        assert_eq!(no_raw_event, AndroidJsonlRecord::NoRawEvent);
    }

    #[test]
    fn parse_line_uses_defaults_for_missing_outer_metadata() {
        let line = r#"{"rawEvent":{"ScreenState":{"timestamp_ms":1234,"state":"Interactive"}}}"#;

        let envelope = AndroidJsonlIngress::new()
            .parse_line(line)
            .unwrap()
            .unwrap();

        assert_eq!(envelope.source, DEFAULT_SOURCE);
        assert_eq!(envelope.device_trace_id, None);
        assert_eq!(envelope.captured_at_ms, 1234);
        assert!(matches!(envelope.raw_event, RawEvent::ScreenState(_)));
    }

    #[test]
    fn parse_line_prefers_outer_timestamp_over_raw_event_timestamp() {
        let line = r#"{"eventId":"evt-screen","timestampMs":9999,"source":"CollectorForegroundService","rawEvent":{"ScreenState":{"timestamp_ms":1234,"state":"Interactive"}}}"#;

        let envelope = AndroidJsonlIngress::new()
            .parse_line(line)
            .unwrap()
            .unwrap();

        assert_eq!(envelope.device_trace_id.as_deref(), Some("evt-screen"));
        assert_eq!(envelope.captured_at_ms, 9999);
    }

    #[test]
    fn parse_line_ignores_blank_source_and_event_id() {
        let line = r#"{"eventId":"   ","timestampMs":1000,"source":"  ","rawEvent":{"ScreenState":{"timestamp_ms":1000,"state":"Interactive"}}}"#;

        let envelope = AndroidJsonlIngress::new()
            .parse_line(line)
            .unwrap()
            .unwrap();

        assert_eq!(envelope.source, DEFAULT_SOURCE);
        assert_eq!(envelope.device_trace_id, None);
    }

    #[test]
    fn promoted_android_sources_parse_as_public_api_ingress() {
        let ingress = AndroidJsonlIngress::new();
        let rows = [
            (APP_TRANSITION_LINE, "UsageCollector"),
            (NOTIFICATION_POSTED_LINE, "NotificationCollectorService"),
            (SYSTEM_STATE_LINE, "CollectorForegroundService"),
        ];

        let envelopes = rows
            .into_iter()
            .map(|(line, source)| {
                let envelope = ingress.parse_line(line).unwrap().unwrap();
                assert_eq!(envelope.source, source);
                assert_eq!(envelope.source_tier, SourceTier::PublicApi);
                envelope
            })
            .collect::<Vec<_>>();

        assert!(matches!(envelopes[0].raw_event, RawEvent::AppTransition(_)));
        match &envelopes[1].raw_event {
            RawEvent::NotificationPosted(event) => {
                assert_eq!(event.package_name, "com.chat");
                assert_eq!(event.raw_text, "sent a file");
            },
            other => panic!("unexpected raw event: {other:?}"),
        }
        match &envelopes[2].raw_event {
            RawEvent::SystemState(event) => {
                assert_eq!(event.battery_pct, Some(88));
                assert_eq!(event.network, NetworkType::Wifi);
            },
            other => panic!("unexpected raw event: {other:?}"),
        }
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

    #[test]
    fn tailer_resets_when_file_is_truncated_or_recreated() {
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

        {
            let mut file = OpenOptions::new()
                .write(true)
                .truncate(true)
                .open(&path)
                .unwrap();
            writeln!(file, "{SYSTEM_STATE_LINE}").unwrap();
        }

        let envelopes = tailer.poll().unwrap();
        assert_eq!(envelopes.len(), 1);
        assert!(matches!(envelopes[0].raw_event, RawEvent::SystemState(_)));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn tailer_keeps_partial_line_across_multiple_polls() {
        let path = temp_trace_path();
        {
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .unwrap();
            write!(file, "{{\"rawEvent\":").unwrap();
        }

        let mut tailer = AndroidJsonlTailer::new(&path);
        assert!(tailer.poll().unwrap().is_empty());

        {
            let mut file = OpenOptions::new().append(true).open(&path).unwrap();
            writeln!(
                file,
                "{{\"ScreenState\":{{\"timestamp_ms\":5000,\"state\":\"Interactive\"}}}}}}"
            )
            .unwrap();
        }

        let envelopes = tailer.poll().unwrap();
        assert_eq!(envelopes.len(), 1);
        assert_eq!(envelopes[0].captured_at_ms, 5000);

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
