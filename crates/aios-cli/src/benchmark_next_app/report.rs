//! Report serialization helpers.

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use anyhow::{Context, Result};

use super::types::BenchmarkReport;

pub const SCHEMA_VERSION: &str = "dipecs.next_app_benchmark.v2";

pub fn write_report(report: &BenchmarkReport, path: &Path) -> Result<()> {
    let file = File::create(path).with_context(|| format!("creating output {}", path.display()))?;
    let writer = BufWriter::new(file);
    serde_json::to_writer_pretty(writer, report)
        .with_context(|| format!("serializing report to {}", path.display()))?;
    Ok(())
}
