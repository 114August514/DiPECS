//! aios-cli — replay/evaluate harness for the DiPECS pipeline.

use std::fs::File;
use std::io::{self, BufReader, BufWriter, Write};
use std::path::PathBuf;

use aios_cli::replay::{self, Stage};
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

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
            let summary = replay::run(reader, &mut sink, window_secs, stages)?;
            sink.flush()?;
            tracing::info!(
                lines_total = summary.lines_total,
                events_ingested = summary.events_ingested,
                windows_closed = summary.windows_closed,
                intents_total = summary.intents_total,
                actions_authorized = summary.actions_authorized,
                "replay complete"
            );
            Ok(())
        },
    }
}
