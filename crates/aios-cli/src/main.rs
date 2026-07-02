//! aios-cli — replay/evaluate harness for the DiPECS pipeline.

use std::fs::File;
use std::io::{self, BufReader, BufWriter, Write};
use std::path::PathBuf;

use aios_cli::android_bridge;
use aios_cli::next_app::{self, NextAppDataset, NextAppSplit};
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
    }
}
