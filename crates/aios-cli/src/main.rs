//! aios-cli — replay/evaluate harness for the DiPECS pipeline.

use std::fs::File;
use std::io::{self, BufReader, BufWriter, Write};
use std::path::PathBuf;

use aios_cli::android_bridge;
use aios_cli::benchmark_next_app::{self, BenchmarkRunConfig};
use aios_cli::next_app::{
    self, build_prewarm_net_benefit_fixture, MeasuredValue, MeasurementSource, NetBenefitTrace,
    NextAppDataset, NextAppSplit, PrewarmFixtureBuildInputs,
};
use aios_cli::replay::{self, Stage};
use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use tracing::info;

const DEFAULT_ANDROID_ACTION_BRIDGE_PORT: u16 = 46321;

#[derive(Parser, Debug)]
#[command(name = "aios-cli", about = "DiPECS internal tooling", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Replay a JSONL trace (Android `CollectorEvent` shape) through the core
    /// pipeline: ingress → sanitize → aggregate → decide → policy → execute.
    Replay {
        /// Path to the JSONL trace file.
        path: PathBuf,

        /// Window aggregation duration in seconds.
        #[arg(long, default_value_t = 10)]
        window_secs: u64,

        /// Highest pipeline stage to run and emit.
        #[arg(long, value_enum, default_value_t = Stage::Policy)]
        stages: Stage,

        /// NDJSON output sink. Defaults to stdout.
        #[arg(long)]
        output: Option<PathBuf>,

        /// Append-only canonical audit log (sorted-key, volatility-stripped
        /// projection of every state transition). When set, the replay also
        /// reports a stable `audit_hash` that can be pinned in regression
        /// tests. Parent directories are created automatically.
        #[arg(long)]
        audit: Option<PathBuf>,
    },

    /// Train a deterministic next-app prediction artifact from an LSApp-shaped dataset.
    TrainNextApp {
        /// Dataset format. Currently supports LSApp CSV/TSV/JSONL.
        #[arg(long, value_enum, default_value_t = NextAppDataset::Lsapp)]
        dataset: NextAppDataset,

        /// External dataset file or directory. The dataset is not committed.
        #[arg(long)]
        input: PathBuf,

        /// Output JSON artifact path.
        #[arg(long)]
        output: PathBuf,

        /// Next-app label horizon in seconds.
        #[arg(long, default_value_t = 30)]
        horizon_secs: u64,

        /// Number of previous app events to expose as model features.
        #[arg(long, default_value_t = 5)]
        history_len: usize,

        /// Train/test split policy used to select training examples.
        #[arg(long, value_enum, default_value_t = NextAppSplit::Standard)]
        split: NextAppSplit,
    },
    /// Evaluate a next-app prediction artifact on an LSApp-shaped dataset.
    EvalNextApp {
        /// Dataset format. Currently supports LSApp CSV/TSV/JSONL.
        #[arg(long, value_enum, default_value_t = NextAppDataset::Lsapp)]
        dataset: NextAppDataset,

        /// External dataset file or directory. The dataset is not committed.
        #[arg(long)]
        input: PathBuf,

        /// Next-app model artifact path.
        #[arg(long)]
        artifact: PathBuf,

        /// Output JSON report path.
        #[arg(long)]
        output: PathBuf,

        /// Next-app label horizon in seconds.
        #[arg(long, default_value_t = 30)]
        horizon_secs: u64,

        /// Number of previous app events to expose as model features.
        #[arg(long, default_value_t = 5)]
        history_len: usize,

        /// Evaluation split policy.
        #[arg(long, value_enum, default_value_t = NextAppSplit::Standard)]
        split: NextAppSplit,
    },
    /// Send a ping/health-check message to the Android localhost socket bridge.
    /// This command does not dispatch any action; it only verifies that the
    /// bridge is reachable and the auth token is accepted.
    SendAuthorizedAction {
        /// Target host. Defaults to Android loopback.
        #[arg(long, default_value = "127.0.0.1")]
        host: String,

        /// Target port. Must match the Android collector socket port.
        #[arg(long, default_value_t = DEFAULT_ANDROID_ACTION_BRIDGE_PORT)]
        port: u16,

        /// Shared auth token required by the Android action socket.
        #[arg(long)]
        auth_token: Option<String>,
    },
    /// Send a real authorized action to the Android action bridge for testing.
    /// Constructs the full payload with HMAC signature and freshness window.
    SendAction {
        /// Target host. Defaults to Android loopback.
        #[arg(long, default_value = "127.0.0.1")]
        host: String,

        /// Target port. Must match the Android collector socket port.
        #[arg(long, default_value_t = DEFAULT_ANDROID_ACTION_BRIDGE_PORT)]
        port: u16,

        /// Shared auth token required by the Android action socket.
        #[arg(long)]
        auth_token: String,

        /// Action type: NoOp, PrefetchFile, KeepAlive, ReleaseMemory, PreWarmProcess.
        #[arg(long, default_value = "NoOp")]
        action_type: String,

        /// Action target (e.g. url:https://..., cache:prefetch, work:collector_heartbeat).
        /// Pass an empty string for no target.
        #[arg(long, default_value = "")]
        target: String,

        /// Action urgency: Immediate, IdleTime, Deferred.
        #[arg(long, default_value = "Immediate")]
        urgency: String,
    },
    /// Benchmark next-app prediction backends against ground-truth labels.
    BenchmarkNextApp {
        /// Input trace JSONL files (one per scenario).
        #[arg(long, required = true)]
        input: Vec<PathBuf>,

        /// Path to the labels JSONL file.
        #[arg(long, required = true)]
        labels: PathBuf,

        /// Output report JSON path.
        #[arg(long, required = true)]
        output: PathBuf,

        /// Fraction of eligible windows (per scenario, time-ordered) to use for training.
        #[arg(long, default_value_t = 0.7)]
        train_split: f64,

        /// Window length in seconds; must match the labels.
        #[arg(long, default_value_t = 10)]
        window_secs: u64,
    },
    /// Generate a PreWarmProcess action-level net-benefit fixture from offline measurements.
    GeneratePrewarmNetBenefitFixture {
        /// LSApp next-app report containing split/test_examples and hit-rate metrics.
        #[arg(long)]
        report: PathBuf,

        /// UX metrics JSON containing ux_deltas.prewarm_vs_cold.startup_total_time_ms_reduction.
        #[arg(long)]
        ux_metrics: PathBuf,

        /// Output fixture JSON path.
        #[arg(long)]
        output: PathBuf,

        /// Dataset id to write into the generated fixture.
        #[arg(long, default_value = "prewarm-offline-generated")]
        dataset_id: String,

        /// Measured missed/wasted PreWarmProcess cost in milliseconds.
        #[arg(long)]
        wasted_prewarm_ms: f64,

        /// Number of samples behind wasted-prewarm measurement.
        #[arg(long, default_value_t = 1)]
        wasted_prewarm_samples: usize,

        /// Optional p95 for wasted-prewarm cost.
        #[arg(long)]
        wasted_prewarm_p95_ms: Option<f64>,

        /// DiPECS control-plane overhead per prediction in milliseconds.
        #[arg(long)]
        dipecs_control_plane_ms: f64,

        /// Number of samples behind DiPECS control-plane measurement.
        #[arg(long, default_value_t = 1)]
        dipecs_control_plane_samples: usize,

        /// Strong baseline control-plane overhead per prediction in milliseconds.
        #[arg(long, default_value_t = 0.0)]
        strong_control_plane_ms: f64,

        /// Number of samples behind strong baseline control-plane measurement.
        #[arg(long, default_value_t = 1)]
        strong_control_plane_samples: usize,
    },
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    match cli.command {
        Command::Replay {
            path,
            window_secs,
            stages,
            output,
            audit,
        } => {
            let file =
                File::open(&path).with_context(|| format!("opening trace {}", path.display()))?;
            let reader = BufReader::new(file);
            let mut sink: Box<dyn Write> = match output {
                Some(p) => Box::new(BufWriter::new(
                    File::create(&p).with_context(|| format!("creating output {}", p.display()))?,
                )),
                None => Box::new(BufWriter::new(io::stdout().lock())),
            };

            let summary = match audit {
                Some(audit_path) => {
                    if let Some(parent) = audit_path.parent() {
                        if !parent.as_os_str().is_empty() {
                            std::fs::create_dir_all(parent).with_context(|| {
                                format!("creating audit dir {}", parent.display())
                            })?;
                        }
                    }
                    let mut audit_sink = BufWriter::new(
                        std::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(&audit_path)
                            .with_context(|| {
                                format!("opening audit file {}", audit_path.display())
                            })?,
                    );
                    let outcome = replay::run_with_audit(
                        reader,
                        &mut sink,
                        &mut audit_sink,
                        window_secs,
                        stages,
                    )?;
                    audit_sink.flush()?;
                    info!(
                        audit_path = %audit_path.display(),
                        audit_hash = %outcome.audit_hash,
                        "audit log written"
                    );
                    outcome.summary
                },
                None => replay::run(reader, &mut sink, window_secs, stages)?,
            };
            sink.flush()?;

            eprintln!("{}", summary.human_summary());

            info!(
                lines_total = summary.lines_total,
                events_ingested = summary.events_ingested,
                windows_closed = summary.windows_closed,
                intents_total = summary.intents_total,
                actions_authorized = summary.actions_authorized,
                audit_hash = %summary.audit_hash,
                "replay complete"
            );
            Ok(())
        },
        Command::TrainNextApp {
            dataset,
            input,
            output,
            horizon_secs,
            history_len,
            split,
        } => next_app::train(next_app::TrainOptions {
            dataset,
            input,
            output,
            horizon_secs,
            history_len,
            split,
        }),
        Command::EvalNextApp {
            dataset,
            input,
            artifact,
            output,
            horizon_secs,
            history_len,
            split,
        } => next_app::evaluate(next_app::EvalOptions {
            dataset,
            input,
            artifact,
            output,
            horizon_secs,
            history_len,
            split,
        }),
        Command::SendAction {
            host,
            port,
            auth_token,
            action_type,
            target,
            urgency,
        } => {
            // Validate action_type is known.
            let valid_types = [
                "NoOp",
                "PrefetchFile",
                "KeepAlive",
                "ReleaseMemory",
                "PreWarmProcess",
            ];
            if !valid_types.contains(&action_type.as_str()) {
                bail!(
                    "unknown action_type '{}'. Valid types: {}",
                    action_type,
                    valid_types.join(", ")
                );
            }
            android_bridge::send_action(&host, port, &auth_token, &action_type, &target, &urgency)?;
            tracing::info!(
                host = %host,
                port,
                action_type = %action_type,
                target = %target,
                urgency = %urgency,
                "authorized action sent to Android bridge"
            );
            Ok(())
        },
        Command::SendAuthorizedAction {
            host,
            port,
            auth_token,
        } => {
            android_bridge::send_ping(&host, port, auth_token.as_deref().unwrap_or(""))?;
            tracing::info!(host = %host, port, "ping sent to Android action bridge");
            Ok(())
        },
        Command::BenchmarkNextApp {
            input,
            labels,
            output,
            train_split,
            window_secs,
        } => {
            if let Some(parent) = output.parent() {
                if !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(parent)
                        .with_context(|| format!("creating output dir {}", parent.display()))?;
                }
            }
            let report = benchmark_next_app::run_benchmark(&BenchmarkRunConfig {
                inputs: input,
                labels,
                train_split,
                window_secs,
            })?;
            benchmark_next_app::report::write_report(&report, &output)
                .with_context(|| format!("writing report {}", output.display()))?;
            info!(
                output = %output.display(),
                scenarios = report.scenarios.len(),
                test_windows = report.test_windows,
                "next-app benchmark complete"
            );
            Ok(())
        },
        Command::GeneratePrewarmNetBenefitFixture {
            report,
            ux_metrics,
            output,
            dataset_id,
            wasted_prewarm_ms,
            wasted_prewarm_samples,
            wasted_prewarm_p95_ms,
            dipecs_control_plane_ms,
            dipecs_control_plane_samples,
            strong_control_plane_ms,
            strong_control_plane_samples,
        } => {
            let fixture = generate_prewarm_net_benefit_fixture(
                &report,
                &ux_metrics,
                dataset_id,
                wasted_prewarm_ms,
                wasted_prewarm_samples,
                wasted_prewarm_p95_ms,
                dipecs_control_plane_ms,
                dipecs_control_plane_samples,
                strong_control_plane_ms,
                strong_control_plane_samples,
            )?;
            if let Some(parent) = output.parent() {
                if !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(parent)
                        .with_context(|| format!("creating output dir {}", parent.display()))?;
                }
            }
            let file = File::create(&output)
                .with_context(|| format!("creating fixture {}", output.display()))?;
            serde_json::to_writer_pretty(BufWriter::new(file), &fixture)
                .with_context(|| format!("writing fixture {}", output.display()))?;
            eprintln!(
                "generated PreWarm net-benefit fixture {} -> {}",
                fixture.dataset_id,
                output.display()
            );
            Ok(())
        },
    }
}

#[allow(clippy::too_many_arguments)]
fn generate_prewarm_net_benefit_fixture(
    report_path: &PathBuf,
    ux_metrics_path: &PathBuf,
    dataset_id: String,
    wasted_prewarm_ms: f64,
    wasted_prewarm_samples: usize,
    wasted_prewarm_p95_ms: Option<f64>,
    dipecs_control_plane_ms: f64,
    dipecs_control_plane_samples: usize,
    strong_control_plane_ms: f64,
    strong_control_plane_samples: usize,
) -> Result<next_app::PrewarmNetBenefitFixture> {
    let report: serde_json::Value = serde_json::from_reader(BufReader::new(
        File::open(report_path)
            .with_context(|| format!("opening report {}", report_path.display()))?,
    ))
    .with_context(|| format!("parsing report {}", report_path.display()))?;
    let ux: serde_json::Value = serde_json::from_reader(BufReader::new(
        File::open(ux_metrics_path)
            .with_context(|| format!("opening UX metrics {}", ux_metrics_path.display()))?,
    ))
    .with_context(|| format!("parsing UX metrics {}", ux_metrics_path.display()))?;

    let split = report
        .get("split")
        .and_then(|v| v.as_str())
        .context("report.split missing")?
        .to_string();
    let examples = report
        .get("test_examples")
        .and_then(|v| v.as_u64())
        .context("report.test_examples missing")? as usize;
    let saved_ms = ux
        .get("ux_deltas")
        .and_then(|d| d.get("prewarm_vs_cold"))
        .and_then(|p| p.get("startup_total_time_ms_reduction"))
        .and_then(|v| v.as_f64())
        .context("ux_deltas.prewarm_vs_cold.startup_total_time_ms_reduction missing")?;
    let saved_samples = startup_samples(&ux).unwrap_or(1);
    let saved_p95 = startup_p95_delta(&ux);

    build_prewarm_net_benefit_fixture(PrewarmFixtureBuildInputs {
        dataset_id,
        status: "generated_from_offline_measurements".into(),
        trace: NetBenefitTrace {
            source: report_path.to_string_lossy().into_owned(),
            split,
            examples,
        },
        prewarm_saved: MeasuredValue {
            mean_ms: saved_ms,
            p95_ms: saved_p95,
            samples: saved_samples,
            source: MeasurementSource {
                kind: "measured_android_emulator_total_time".into(),
                path: ux_metrics_path.to_string_lossy().into_owned(),
                field: Some("ux_deltas.prewarm_vs_cold.startup_total_time_ms_reduction".into()),
                note: Some("Loaded from committed UX metrics fixture".into()),
            },
        },
        wasted_prewarm: MeasuredValue {
            mean_ms: wasted_prewarm_ms,
            p95_ms: wasted_prewarm_p95_ms,
            samples: wasted_prewarm_samples,
            source: MeasurementSource {
                kind: "offline_measured_wasted_prewarm_cost".into(),
                path: report_path.to_string_lossy().into_owned(),
                field: Some("cli.--wasted-prewarm-ms".into()),
                note: Some("Provided explicitly to offline fixture generator".into()),
            },
        },
        dipecs_control_plane: MeasuredValue {
            mean_ms: dipecs_control_plane_ms,
            p95_ms: None,
            samples: dipecs_control_plane_samples,
            source: MeasurementSource {
                kind: "offline_measured_control_plane_cost".into(),
                path: report_path.to_string_lossy().into_owned(),
                field: Some("cli.--dipecs-control-plane-ms".into()),
                note: Some("Provided explicitly to offline fixture generator".into()),
            },
        },
        strong_control_plane: MeasuredValue {
            mean_ms: strong_control_plane_ms,
            p95_ms: None,
            samples: strong_control_plane_samples,
            source: MeasurementSource {
                kind: "offline_measured_or_favorable_baseline_control_plane_cost".into(),
                path: report_path.to_string_lossy().into_owned(),
                field: Some("cli.--strong-control-plane-ms".into()),
                note: Some("Provided explicitly to offline fixture generator".into()),
            },
        },
    })
    .map_err(anyhow::Error::msg)
}

fn startup_samples(ux: &serde_json::Value) -> Option<usize> {
    let runs = ux.get("runs")?.as_array()?;
    let mut samples = 0usize;
    for mode in ["cold_startup", "prewarm_startup"] {
        let run = runs
            .iter()
            .find(|run| run.get("mode").and_then(|v| v.as_str()) == Some(mode))?;
        samples += run.get("samples")?.as_array()?.len();
    }
    Some(samples)
}

fn startup_p95_delta(ux: &serde_json::Value) -> Option<f64> {
    let runs = ux.get("runs")?.as_array()?;
    let p95 = |mode: &str| {
        runs.iter()
            .find(|run| run.get("mode").and_then(|v| v.as_str()) == Some(mode))?
            .get("summary")?
            .get("p95_startup_total_time_ms")?
            .as_f64()
    };
    Some((p95("cold_startup")? - p95("prewarm_startup")?).max(0.0))
}
