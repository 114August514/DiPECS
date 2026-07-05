use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn evaluation_dir() -> PathBuf {
    workspace_root().join("data/evaluation")
}

pub(super) fn find_report() -> Option<PathBuf> {
    let path = evaluation_dir()
        .join("next-app")
        .join("lsapp-standard.report.json");
    path.exists().then_some(path)
}

pub(super) fn find_net_benefit_report() -> Option<PathBuf> {
    let path = evaluation_dir()
        .join("next-app")
        .join("prewarm-net-benefit-real-device-20260704-184148.json");
    path.exists().then_some(path)
}

pub(super) fn find_keepalive_memory_pressure_report() -> Option<PathBuf> {
    let path = evaluation_dir()
        .join("keepalive")
        .join("keepalive-memory-pressure-real-device-20260705-fixture.json");
    path.exists().then_some(path)
}

pub(super) fn find_ux_metrics() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = fs::read_dir(evaluation_dir().join("ux-metrics"))
        .ok()?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let name = entry.file_name().into_string().ok()?;
            if name.starts_with("ux-metrics-emulator-") && name.ends_with(".json") {
                Some(entry.path())
            } else {
                None
            }
        })
        .collect();
    candidates.sort();
    candidates.into_iter().last()
}

pub(super) fn find_prewarm_net_benefit_fixture() -> Option<PathBuf> {
    let path = evaluation_dir()
        .join("action-net-benefit")
        .join("prewarm-emulator-20260704-measured-v1.json");
    path.exists().then_some(path)
}

pub(super) fn load_json(path: &PathBuf) -> Option<serde_json::Value> {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("SKIP: could not read {}: {e}", path.display());
            return None;
        },
    };
    match serde_json::from_str(&text) {
        Ok(v) => Some(v),
        Err(e) => {
            eprintln!("SKIP: could not parse {}: {e}", path.display());
            None
        },
    }
}

pub(super) fn unique_tmp_fixture_path() -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir()
        .join(format!("dipecs-prewarm-net-benefit-{suffix}"))
        .join("fixture.json")
}

pub(super) fn unique_tmp_dir(prefix: &str) -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{suffix}"))
}

pub(super) fn run_fixture_generator(
    report: &PathBuf,
    ux_metrics: &PathBuf,
    output: &PathBuf,
) -> bool {
    Command::new(env!("CARGO_BIN_EXE_aios-cli"))
        .arg("generate-prewarm-net-benefit-fixture")
        .arg("--report")
        .arg(report)
        .arg("--ux-metrics")
        .arg(ux_metrics)
        .arg("--output")
        .arg(output)
        .arg("--wasted-prewarm-ms")
        .arg("31.231")
        .arg("--wasted-prewarm-samples")
        .arg("1")
        .arg("--dipecs-control-plane-ms")
        .arg("0.07848")
        .arg("--dipecs-control-plane-samples")
        .arg("1631")
        .arg("--strong-control-plane-ms")
        .arg("0.0")
        .arg("--strong-control-plane-samples")
        .arg("272519")
        .status()
        .expect("aios-cli should run")
        .success()
}
