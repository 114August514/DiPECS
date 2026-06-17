//! 验证 ProcessingEvent dispatch 逻辑。

use std::io::Read;
use std::time::{SystemTime, UNIX_EPOCH};

use aios_daemon::pipeline::{should_stop_processing, ProcessingEvent, RuntimeTraceRecorder};
use serde_json::Value;

#[test]
fn window_expiry_does_not_stop_processing_loop() {
    assert!(!should_stop_processing(&ProcessingEvent::WindowExpired));
}

#[test]
fn closed_raw_channel_stops_processing_loop() {
    assert!(should_stop_processing(&ProcessingEvent::RawChannelClosed));
}

#[test]
fn runtime_trace_recorder_writes_ndjson() {
    let path = std::env::temp_dir().join(format!(
        "dipecs-runtime-trace-{}.ndjson",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut recorder = RuntimeTraceRecorder::new(&path).unwrap();
    recorder
        .record_window(&serde_json::json!({
            "stage": "daemon_window",
            "window_id": "test-window",
        }))
        .unwrap();
    drop(recorder);

    let mut content = String::new();
    std::fs::File::open(&path)
        .unwrap()
        .read_to_string(&mut content)
        .unwrap();
    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines.len(), 1);

    let parsed: Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(parsed["stage"], "daemon_window");
    assert_eq!(parsed["window_id"], "test-window");

    let _ = std::fs::remove_file(path);
}
