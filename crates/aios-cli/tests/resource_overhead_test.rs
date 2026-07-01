//! 资源开销测试:测量 replay 大 trace 时的 wall time、峰值 RSS、CPU 时间、吞吐。
//!
//! 只读 /proc/self/*,不修改系统状态。Linux only(符合 DiPECS 目标平台)。

use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::time::Instant;

use aios_cli::replay::{run, Stage};

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("resolve project root")
}

fn read_peak_rss_kb() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    status
        .lines()
        .find(|l| l.starts_with("VmHWM:"))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
}

fn read_cpu_times() -> Option<(u64, u64)> {
    // /proc/self/stat: ... utime stime ... (field 14 and 15, 0-indexed 13/14)
    let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
    let parts: Vec<&str> = stat.split_whitespace().collect();
    let utime: u64 = parts.get(13)?.parse().ok()?;
    let stime: u64 = parts.get(14)?.parse().ok()?;
    Some((utime, stime))
}

fn cpu_clock_ticks_per_sec() -> u64 {
    // Linux _SC_CLK_TCK is virtually always 100; avoid pulling in libc for a test.
    // If paranoid, run `getconf CLK_TCK` at test time.
    if let Ok(out) = std::process::Command::new("getconf")
        .arg("CLK_TCK")
        .output()
    {
        if out.status.success() {
            if let Ok(s) = std::str::from_utf8(&out.stdout) {
                if let Ok(v) = s.trim().parse::<u64>() {
                    return v;
                }
            }
        }
    }
    100
}

#[test]
#[cfg(target_os = "linux")]
fn replay_large_trace_resource_overhead() {
    let root = project_root();
    let trace = root.join("data/traces/android_synthetic_large.redacted.jsonl");
    assert!(trace.exists(), "large trace missing: {}", trace.display());

    println!("\n=== resource overhead: replay {} ===", trace.display());

    let file = File::open(&trace).expect("open trace");
    let reader = BufReader::new(file);
    let mut writer = Vec::new();

    let start_wall = Instant::now();
    let (start_utime, start_stime) = read_cpu_times().expect("read /proc/self/stat");

    let summary = run(reader, &mut writer, 60, Stage::Execute).expect("replay should succeed");

    let wall_ms = start_wall.elapsed().as_millis() as f64;
    let (end_utime, end_stime) = read_cpu_times().expect("read /proc/self/stat");
    let peak_rss_kb = read_peak_rss_kb().expect("read /proc/self/status");

    let ticks = cpu_clock_ticks_per_sec();
    let user_ms = (end_utime.saturating_sub(start_utime) as f64 / ticks as f64) * 1000.0;
    let sys_ms = (end_stime.saturating_sub(start_stime) as f64 / ticks as f64) * 1000.0;
    let throughput = summary.events_ingested as f64 / (wall_ms / 1000.0);

    println!("wall_time_ms        : {wall_ms:.1}");
    println!("peak_rss_mb         : {:.2}", peak_rss_kb as f64 / 1024.0);
    println!("cpu_user_ms         : {user_ms:.1}");
    println!("cpu_system_ms       : {sys_ms:.1}");
    println!("events_ingested     : {}", summary.events_ingested);
    println!("windows_closed      : {}", summary.windows_closed);
    println!("actions_authorized  : {}", summary.actions_authorized);
    println!("throughput_events/s : {throughput:.1}");
    println!("audit_hash          : {}", summary.audit_hash);

    assert!(
        summary.events_ingested > 1000,
        "expected to ingest >1000 events, got {}",
        summary.events_ingested
    );
    assert!(
        peak_rss_kb < 512 * 1024,
        "peak RSS should stay under 512 MiB, got {peak_rss_kb} KiB"
    );
}
