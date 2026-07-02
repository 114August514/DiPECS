//! Shared helpers for baseline tests.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

#[allow(dead_code)] // TODO: remove once Tasks 1-8 start using this helper.
pub fn repo_root() -> PathBuf {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("manifest inside tests/integration")
        .parent()
        .expect("manifest inside repo")
        .to_path_buf();
    assert!(
        root.join("Cargo.toml").exists(),
        "repo root should contain Cargo.toml: {}",
        root.display()
    );
    root
}

#[allow(dead_code)] // TODO: remove once Tasks 1-8 start using this helper.
pub fn load_jsonl_events(path: &str) -> Vec<serde_json::Value> {
    let file = File::open(path).unwrap_or_else(|e| panic!("open trace {}: {}", path, e));
    BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            serde_json::from_str(&l).unwrap_or_else(|e| panic!("invalid JSON in {}: {}", path, e))
        })
        .collect()
}

/// Read peak resident set size (VmHWM) from `/proc/self/status` in KiB.
///
/// Linux only; returns `None` if the file is unavailable or the field is missing.
pub fn read_peak_rss_kb() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    status
        .lines()
        .find(|l| l.starts_with("VmHWM:"))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
}

/// Read user/system CPU time ticks from `/proc/self/stat`.
///
/// Fields 14 and 15 (0-indexed 13/14) are utime and stime.
/// Linux only; returns `None` if the file is unavailable or malformed.
pub fn read_cpu_times_ticks() -> Option<(u64, u64)> {
    let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
    let parts: Vec<&str> = stat.split_whitespace().collect();
    let utime: u64 = parts.get(13)?.parse().ok()?;
    let stime: u64 = parts.get(14)?.parse().ok()?;
    Some((utime, stime))
}

/// Return the kernel's `CLK_TCK` value used for `/proc/self/stat` CPU times.
///
/// Defaults to 100 (the overwhelmingly common Linux value) if `getconf` fails.
pub fn clock_ticks_per_sec() -> u64 {
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
