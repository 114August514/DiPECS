//! 窗口大小 baseline：不同 window_secs 下的 replay 性能与资源开销。

use aios_cli::replay::{self, Stage};
use std::fs::File;
use std::io::BufReader;
use std::time::Instant;

use crate::helpers;

#[derive(Debug)]
struct WindowResult {
    events: u64,
    wall_ms: u128,
    peak_rss_kb: u64,
    cpu_total_ms: f64,
}

fn replay_trace(path: &str, window_secs: u64) -> WindowResult {
    let file = File::open(path).unwrap();
    let reader = BufReader::new(file);
    let mut sink = std::io::sink();

    let start_cpu = helpers::read_cpu_times_ticks().expect("read /proc/self/stat");
    let start = Instant::now();
    let summary = replay::run(reader, &mut sink, window_secs, Stage::Execute).unwrap();
    let wall_ms = start.elapsed().as_millis();
    let end_cpu = helpers::read_cpu_times_ticks().expect("read /proc/self/stat");
    let peak_rss_kb = helpers::read_peak_rss_kb().expect("read /proc/self/status");

    let ticks = helpers::clock_ticks_per_sec() as f64;
    let user_ms = (end_cpu.0.saturating_sub(start_cpu.0) as f64 / ticks) * 1000.0;
    let sys_ms = (end_cpu.1.saturating_sub(start_cpu.1) as f64 / ticks) * 1000.0;

    WindowResult {
        events: summary.events_ingested,
        wall_ms,
        peak_rss_kb,
        cpu_total_ms: user_ms + sys_ms,
    }
}

#[test]
#[cfg(target_os = "linux")]
fn larger_windows_preserve_throughput_and_resources() {
    let path = helpers::repo_root()
        .join("data/traces/android_synthetic_large.redacted.jsonl")
        .to_str()
        .unwrap()
        .to_string();

    let r1 = replay_trace(&path, 1);
    let r10 = replay_trace(&path, 10);
    let r60 = replay_trace(&path, 60);

    assert_eq!(r1.events, r10.events);
    assert_eq!(r10.events, r60.events);

    // Guard against division-by-zero and trivial runs before computing ratios.
    assert!(
        r1.events > 1000,
        "expected to ingest >1000 events, got {}",
        r1.events
    );
    assert!(
        r1.wall_ms > 0,
        "expected 1s window to take >0 ms, got {}",
        r1.wall_ms
    );

    let throughput_1 = r1.events as f64 / r1.wall_ms as f64;
    let throughput_10 = r10.events as f64 / r10.wall_ms.max(1) as f64;
    let throughput_60 = r60.events as f64 / r60.wall_ms.max(1) as f64;

    eprintln!("\n=== window size baseline ===");
    eprintln!(
        "1s  window: events={} wall_ms={} throughput={:.2} ev/ms peak_rss={} KiB cpu_total={:.1} ms",
        r1.events, r1.wall_ms, throughput_1, r1.peak_rss_kb, r1.cpu_total_ms
    );
    eprintln!(
        "10s window: events={} wall_ms={} throughput={:.2} ev/ms peak_rss={} KiB cpu_total={:.1} ms",
        r10.events, r10.wall_ms, throughput_10, r10.peak_rss_kb, r10.cpu_total_ms
    );
    eprintln!(
        "60s window: events={} wall_ms={} throughput={:.2} ev/ms peak_rss={} KiB cpu_total={:.1} ms",
        r60.events, r60.wall_ms, throughput_60, r60.peak_rss_kb, r60.cpu_total_ms
    );

    // Tightened regression guards for larger windows.
    const THROUGHPUT_10_MIN_RATIO: f64 = 0.85;
    const THROUGHPUT_60_MIN_RATIO: f64 = 0.65;
    const RSS_MAX_RATIO: f64 = 1.5;
    const CPU_MAX_RATIO: f64 = 1.5;

    assert!(
        throughput_10 >= throughput_1 * THROUGHPUT_10_MIN_RATIO,
        "10s window throughput should not be much worse than 1s: {:.2} vs {:.2}",
        throughput_10,
        throughput_1
    );
    assert!(
        throughput_60 >= throughput_1 * THROUGHPUT_60_MIN_RATIO,
        "60s window throughput should remain reasonable: {:.2} vs {:.2}",
        throughput_60,
        throughput_1
    );

    assert!(
        r10.peak_rss_kb as f64 <= r1.peak_rss_kb as f64 * RSS_MAX_RATIO,
        "10s window peak RSS should be within {:.1}x of 1s: {} KiB vs {} KiB",
        RSS_MAX_RATIO,
        r10.peak_rss_kb,
        r1.peak_rss_kb
    );
    assert!(
        r60.peak_rss_kb as f64 <= r1.peak_rss_kb as f64 * RSS_MAX_RATIO,
        "60s window peak RSS should be within {:.1}x of 1s: {} KiB vs {} KiB",
        RSS_MAX_RATIO,
        r60.peak_rss_kb,
        r1.peak_rss_kb
    );
    assert!(
        r10.cpu_total_ms <= r1.cpu_total_ms * CPU_MAX_RATIO,
        "10s window CPU time should be within {:.1}x of 1s: {:.1} ms vs {:.1} ms",
        CPU_MAX_RATIO,
        r10.cpu_total_ms,
        r1.cpu_total_ms
    );
    assert!(
        r60.cpu_total_ms <= r1.cpu_total_ms * CPU_MAX_RATIO,
        "60s window CPU time should be within {:.1}x of 1s: {:.1} ms vs {:.1} ms",
        CPU_MAX_RATIO,
        r60.cpu_total_ms,
        r1.cpu_total_ms
    );
}
