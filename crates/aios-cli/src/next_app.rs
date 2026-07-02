//! Offline next-app training/evaluation helpers for LSApp-shaped datasets.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, BufWriter};
use std::path::{Path, PathBuf};

use aios_agent::{
    prediction_features_for_example, train_next_app_artifact, NextAppAlgorithm,
    NextAppModelArtifact, NextAppModelConfig, NextAppPredictor, NextAppTrainingExample,
};
use anyhow::{bail, Context, Result};
use clap::ValueEnum;
use serde::Serialize;

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum NextAppDataset {
    Lsapp,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum NextAppSplit {
    Standard,
    ColdStart,
}

#[derive(Debug, Clone)]
pub struct TrainOptions {
    pub dataset: NextAppDataset,
    pub input: PathBuf,
    pub output: PathBuf,
    pub horizon_secs: u64,
    pub history_len: usize,
    pub split: NextAppSplit,
}

#[derive(Debug, Clone)]
pub struct EvalOptions {
    pub dataset: NextAppDataset,
    pub input: PathBuf,
    pub artifact: PathBuf,
    pub output: PathBuf,
    pub horizon_secs: u64,
    pub history_len: usize,
    pub split: NextAppSplit,
}

#[derive(Debug, Clone)]
struct LsAppRecord {
    user_id: String,
    session_id: String,
    timestamp_ms: i64,
    app_name: String,
    event_type: String,
    ordinal: usize,
}

#[derive(Debug, Serialize)]
struct EvalReport {
    schema_version: String,
    dataset: String,
    split: String,
    artifact_model_id: String,
    artifact_dataset_id: String,
    total_examples: usize,
    train_examples: usize,
    test_examples: usize,
    metrics: BTreeMap<String, MetricsReport>,
}

#[derive(Debug, Default, Clone, Serialize)]
struct MetricsReport {
    examples: usize,
    predicted: usize,
    hit_rate_at_1_pct: f32,
    hit_rate_at_3_pct: f32,
    hit_rate_at_5_pct: f32,
    mean_reciprocal_rank_at_5: f32,
    prediction_coverage_pct: f32,
    macro_hit_rate_at_1_pct: f32,
}

#[derive(Debug, Default, Clone)]
struct MetricsAccum {
    examples: usize,
    predicted: usize,
    hit1: usize,
    hit3: usize,
    hit5: usize,
    mrr5: f32,
    per_user: BTreeMap<String, UserAccum>,
}

#[derive(Debug, Default, Clone)]
struct UserAccum {
    examples: usize,
    hit1: usize,
}

pub fn train(opts: TrainOptions) -> Result<()> {
    let examples = load_examples(&opts.input, opts.horizon_secs, opts.history_len)
        .with_context(|| format!("loading LSApp dataset from {}", opts.input.display()))?;
    let (train_examples, _) = split_examples(&examples, opts.split);
    let config = NextAppModelConfig {
        horizon_secs: opts.horizon_secs,
        history_len: opts.history_len,
    };
    let artifact =
        train_next_app_artifact("lsapp", config, &train_examples).map_err(anyhow::Error::msg)?;

    if let Some(parent) = opts.output.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .with_context(|| format!("creating output dir {}", parent.display()))?;
        }
    }
    let file = File::create(&opts.output)
        .with_context(|| format!("creating artifact {}", opts.output.display()))?;
    serde_json::to_writer_pretty(BufWriter::new(file), &artifact)
        .with_context(|| format!("writing artifact {}", opts.output.display()))?;
    eprintln!(
        "trained next-app artifact: {} examples, {} apps -> {}",
        artifact.training_summary.examples,
        artifact.training_summary.apps,
        opts.output.display()
    );
    Ok(())
}

pub fn evaluate(opts: EvalOptions) -> Result<()> {
    let artifact_file = File::open(&opts.artifact)
        .with_context(|| format!("opening artifact {}", opts.artifact.display()))?;
    let artifact: NextAppModelArtifact = serde_json::from_reader(BufReader::new(artifact_file))
        .with_context(|| format!("parsing artifact {}", opts.artifact.display()))?;
    let predictor = NextAppPredictor::new(artifact.clone()).map_err(anyhow::Error::msg)?;

    let examples = load_examples(&opts.input, opts.horizon_secs, opts.history_len)
        .with_context(|| format!("loading LSApp dataset from {}", opts.input.display()))?;
    let (train_examples, test_examples) = split_examples(&examples, opts.split);
    if test_examples.is_empty() {
        bail!("split produced zero test examples");
    }
    let baseline = BaselineTables::from_training(&train_examples);

    let mut metrics = BTreeMap::new();
    metrics.insert(
        "global_popularity".into(),
        evaluate_ranker(&test_examples, |_example| {
            baseline.global_popularity.iter().take(5).cloned().collect()
        }),
    );
    metrics.insert(
        "mfu".into(),
        evaluate_ranker(&test_examples, |example| baseline.mfu(&example.user_id)),
    );
    metrics.insert(
        "mru".into(),
        evaluate_ranker(&test_examples, |example| {
            example
                .history
                .last()
                .cloned()
                .map(|app| vec![app])
                .unwrap_or_default()
        }),
    );
    for (name, algorithm) in [
        ("naive_bayes", NextAppAlgorithm::NaiveBayes),
        ("markov", NextAppAlgorithm::Markov),
        // The report key remains "xgboost" for backward compatibility with
        // committed baseline reports; the underlying model is a log-lift
        // feature ensemble, not XGBoost.
        ("xgboost", NextAppAlgorithm::FeatureLift),
        ("ensemble", NextAppAlgorithm::Ensemble),
    ] {
        metrics.insert(
            name.into(),
            evaluate_ranker(&test_examples, |example| {
                predictor
                    .rank(&prediction_features_for_example(example), algorithm, 5)
                    .into_iter()
                    .map(|score| score.app)
                    .collect()
            }),
        );
    }

    let report = EvalReport {
        schema_version: "dipecs.next_app_eval.v1".into(),
        dataset: format!("{:?}", opts.dataset),
        split: format!("{:?}", opts.split),
        artifact_model_id: artifact.model_id,
        artifact_dataset_id: artifact.dataset_id,
        total_examples: examples.len(),
        train_examples: train_examples.len(),
        test_examples: test_examples.len(),
        metrics,
    };

    if let Some(parent) = opts.output.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .with_context(|| format!("creating output dir {}", parent.display()))?;
        }
    }
    let file = File::create(&opts.output)
        .with_context(|| format!("creating report {}", opts.output.display()))?;
    serde_json::to_writer_pretty(BufWriter::new(file), &report)
        .with_context(|| format!("writing report {}", opts.output.display()))?;
    eprintln!(
        "evaluated next-app artifact on {} test examples -> {}",
        report.test_examples,
        opts.output.display()
    );
    Ok(())
}

fn load_examples(
    path: &Path,
    horizon_secs: u64,
    history_len: usize,
) -> Result<Vec<NextAppTrainingExample>> {
    let records = load_records(path)?;
    if records.len() < 2 {
        bail!("LSApp input has fewer than two records");
    }

    let mut by_user: BTreeMap<String, Vec<LsAppRecord>> = BTreeMap::new();
    for record in records {
        by_user
            .entry(record.user_id.clone())
            .or_default()
            .push(record);
    }

    let mut examples = Vec::new();
    for (user_id, mut records) in by_user {
        records.sort_by(|a, b| {
            a.session_id
                .cmp(&b.session_id)
                .then(a.timestamp_ms.cmp(&b.timestamp_ms))
                .then(a.ordinal.cmp(&b.ordinal))
        });
        let mut history: VecDeque<String> = VecDeque::new();
        for idx in 0..records.len().saturating_sub(1) {
            let current = &records[idx];
            let next = match next_label_record(&records, idx, horizon_secs) {
                Some(next) => next,
                None => {
                    push_history(&mut history, &current.app_name, history_len);
                    continue;
                },
            };
            if current.app_name != next.app_name {
                examples.push(NextAppTrainingExample {
                    user_id: user_id.clone(),
                    current_app: current.app_name.clone(),
                    history: history.iter().cloned().collect(),
                    hour_bucket: hour_bucket(current.timestamp_ms),
                    weekday: weekday(current.timestamp_ms),
                    event_type: current.event_type.clone(),
                    label_app: next.app_name.clone(),
                });
            }
            push_history(&mut history, &current.app_name, history_len);
        }
    }
    Ok(examples)
}

fn next_label_record(
    records: &[LsAppRecord],
    idx: usize,
    horizon_secs: u64,
) -> Option<&LsAppRecord> {
    let current = &records[idx];
    records[idx + 1..]
        .iter()
        .take_while(|candidate| candidate.session_id == current.session_id)
        // Records are sorted by timestamp within a session, so the delta is
        // non-negative; the previous `<= current` disjunct was redundant.
        .take_while(|candidate| {
            candidate.timestamp_ms - current.timestamp_ms <= horizon_secs as i64 * 1000
        })
        .find(|candidate| candidate.app_name != current.app_name)
}

fn push_history(history: &mut VecDeque<String>, app: &str, history_len: usize) {
    history.push_back(app.to_string());
    while history.len() > history_len {
        history.pop_front();
    }
}

fn split_examples(
    examples: &[NextAppTrainingExample],
    split: NextAppSplit,
) -> (Vec<NextAppTrainingExample>, Vec<NextAppTrainingExample>) {
    match split {
        NextAppSplit::Standard => split_standard(examples),
        NextAppSplit::ColdStart => split_cold_start(examples),
    }
}

fn split_standard(
    examples: &[NextAppTrainingExample],
) -> (Vec<NextAppTrainingExample>, Vec<NextAppTrainingExample>) {
    let mut train = Vec::new();
    let mut test = Vec::new();
    let mut by_user: BTreeMap<&str, Vec<&NextAppTrainingExample>> = BTreeMap::new();
    for example in examples {
        by_user.entry(&example.user_id).or_default().push(example);
    }
    for (_, user_examples) in by_user {
        let cutoff = ((user_examples.len() as f32) * 0.8).floor() as usize;
        for (idx, example) in user_examples.into_iter().enumerate() {
            if idx < cutoff.max(1) {
                train.push(example.clone());
            } else {
                test.push(example.clone());
            }
        }
    }
    (train, test)
}

fn split_cold_start(
    examples: &[NextAppTrainingExample],
) -> (Vec<NextAppTrainingExample>, Vec<NextAppTrainingExample>) {
    let users: BTreeSet<&str> = examples
        .iter()
        .map(|example| example.user_id.as_str())
        .collect();
    let cutoff = ((users.len() as f32) * 0.8).floor() as usize;
    let train_users: BTreeSet<&str> = users.iter().take(cutoff.max(1)).copied().collect();
    let mut train = Vec::new();
    let mut test = Vec::new();
    for example in examples {
        if train_users.contains(example.user_id.as_str()) {
            train.push(example.clone());
        } else {
            test.push(example.clone());
        }
    }
    (train, test)
}

fn load_records(path: &Path) -> Result<Vec<LsAppRecord>> {
    let mut files = Vec::new();
    collect_input_files(path, &mut files)?;
    let mut records = Vec::new();
    for file in files {
        records.extend(load_record_file(&file)?);
    }
    Ok(records)
}

fn collect_input_files(path: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    if path.is_file() {
        out.push(path.to_path_buf());
        return Ok(());
    }
    for entry in fs::read_dir(path).with_context(|| format!("reading dir {}", path.display()))? {
        let entry = entry?;
        let child = entry.path();
        if child.is_dir() {
            collect_input_files(&child, out)?;
        } else if matches!(
            child.extension().and_then(|ext| ext.to_str()),
            Some("csv" | "tsv" | "jsonl" | "json")
        ) {
            out.push(child);
        }
    }
    Ok(())
}

fn load_record_file(path: &Path) -> Result<Vec<LsAppRecord>> {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("jsonl") => load_jsonl(path),
        _ => load_delimited(path),
    }
}

fn load_jsonl(path: &Path) -> Result<Vec<LsAppRecord>> {
    let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let reader = BufReader::new(file);
    let mut records = Vec::new();
    for (ordinal, line) in reader.lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let value: serde_json::Value = serde_json::from_str(&line)
            .with_context(|| format!("parsing JSONL {} line {}", path.display(), ordinal + 1))?;
        records.push(record_from_map(&JsonMap(value), ordinal)?);
    }
    Ok(records)
}

fn load_delimited(path: &Path) -> Result<Vec<LsAppRecord>> {
    let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut lines = BufReader::new(file).lines();
    let Some(header_line) = lines.next() else {
        return Ok(Vec::new());
    };
    let header_line = header_line?;
    let delimiter = if header_line.contains('\t') {
        '\t'
    } else {
        ','
    };
    let headers: Vec<String> = split_delimited(&header_line, delimiter)
        .into_iter()
        .map(|s| s.to_lowercase())
        .collect();
    let mut records = Vec::new();
    for (ordinal, line) in lines.enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let fields = split_delimited(&line, delimiter);
        let row = DelimitedRow {
            headers: &headers,
            fields: &fields,
        };
        records.push(record_from_map(&row, ordinal)?);
    }
    Ok(records)
}

trait RecordMap {
    fn get(&self, candidates: &[&str]) -> Option<String>;
}

struct DelimitedRow<'a> {
    headers: &'a [String],
    fields: &'a [String],
}

impl RecordMap for DelimitedRow<'_> {
    fn get(&self, candidates: &[&str]) -> Option<String> {
        candidates.iter().find_map(|candidate| {
            self.headers
                .iter()
                .position(|header| header == candidate)
                .and_then(|idx| self.fields.get(idx).cloned())
        })
    }
}

struct JsonMap(serde_json::Value);

impl RecordMap for JsonMap {
    fn get(&self, candidates: &[&str]) -> Option<String> {
        candidates.iter().find_map(|candidate| {
            self.0.get(*candidate).and_then(|value| match value {
                serde_json::Value::String(s) => Some(s.clone()),
                serde_json::Value::Number(n) => Some(n.to_string()),
                _ => None,
            })
        })
    }
}

fn record_from_map(row: &dyn RecordMap, ordinal: usize) -> Result<LsAppRecord> {
    let user_id = required(row, &["user_id", "userid", "user", "uid"])?;
    let session_id = row
        .get(&["session_id", "sessionid", "session"])
        .unwrap_or_else(|| "default".into());
    let app_name = required(
        row,
        &["app_name", "appname", "app", "package", "package_name"],
    )?;
    let event_type = row
        .get(&["event_type", "event", "type"])
        .unwrap_or_else(|| "app_usage".into());
    let timestamp_ms = row
        .get(&["timestamp_ms"])
        .and_then(|value| value.trim().parse::<i64>().ok())
        .or_else(|| {
            row.get(&["timestamp", "time", "ts"])
                .and_then(|value| parse_timestamp_ms(&value))
        })
        .unwrap_or(ordinal as i64 * 1000);
    Ok(LsAppRecord {
        user_id,
        session_id,
        timestamp_ms,
        app_name,
        event_type,
        ordinal,
    })
}

fn required(row: &dyn RecordMap, candidates: &[&str]) -> Result<String> {
    row.get(candidates)
        .filter(|value| !value.trim().is_empty())
        .with_context(|| format!("missing required column; tried {candidates:?}"))
}

fn parse_timestamp_ms(raw: &str) -> Option<i64> {
    let trimmed = raw.trim();
    let value = trimmed.parse::<i64>().ok()?;
    // Heuristic for the ambiguous "timestamp" column (as opposed to the
    // unambiguous "timestamp_ms" column, which is parsed directly as ms).
    //
    // - Values >= 1_000_000_000_000 (~2001-09-09 in ms) are treated as ms.
    // - Smaller values are treated as seconds and multiplied by 1000.
    //
    // This is not perfect: an early-ms timestamp between 1970 and 2001 would
    // be misclassified as seconds. Datasets that need exact semantics should
    // use a column named `timestamp_ms`.
    const MS_THRESHOLD: i64 = 1_000_000_000_000;
    Some(if value >= MS_THRESHOLD {
        value
    } else {
        value * 1000
    })
}

fn split_delimited(line: &str, delimiter: char) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '"' if in_quotes && chars.peek() == Some(&'"') => {
                current.push('"');
                chars.next();
            },
            '"' => in_quotes = !in_quotes,
            ch if ch == delimiter && !in_quotes => {
                fields.push(current.trim().to_string());
                current.clear();
            },
            _ => current.push(ch),
        }
    }
    fields.push(current.trim().to_string());
    fields
}

struct BaselineTables {
    global_popularity: Vec<String>,
    user_frequency: BTreeMap<String, Vec<String>>,
}

impl BaselineTables {
    fn from_training(examples: &[NextAppTrainingExample]) -> Self {
        let mut global_counts: HashMap<String, u32> = HashMap::new();
        let mut user_counts: BTreeMap<String, HashMap<String, u32>> = BTreeMap::new();
        for example in examples {
            *global_counts.entry(example.label_app.clone()).or_default() += 1;
            *user_counts
                .entry(example.user_id.clone())
                .or_default()
                .entry(example.label_app.clone())
                .or_default() += 1;
        }
        Self {
            global_popularity: rank_counts(global_counts),
            user_frequency: user_counts
                .into_iter()
                .map(|(user, counts)| (user, rank_counts(counts)))
                .collect(),
        }
    }

    fn mfu(&self, user_id: &str) -> Vec<String> {
        self.user_frequency
            .get(user_id)
            .cloned()
            .unwrap_or_else(|| self.global_popularity.clone())
            .into_iter()
            .take(5)
            .collect()
    }
}

fn rank_counts(counts: HashMap<String, u32>) -> Vec<String> {
    let mut ranked: Vec<(String, u32)> = counts.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    ranked.into_iter().map(|(app, _)| app).collect()
}

fn evaluate_ranker<F>(examples: &[NextAppTrainingExample], mut ranker: F) -> MetricsReport
where
    F: FnMut(&NextAppTrainingExample) -> Vec<String>,
{
    let mut accum = MetricsAccum::default();
    for example in examples {
        accum.examples += 1;
        let predictions = ranker(example);
        if !predictions.is_empty() {
            accum.predicted += 1;
        }
        let rank = predictions
            .iter()
            .take(5)
            .position(|app| app == &example.label_app)
            .map(|idx| idx + 1);
        if rank == Some(1) {
            accum.hit1 += 1;
        }
        if rank.is_some_and(|rank| rank <= 3) {
            accum.hit3 += 1;
        }
        if let Some(rank) = rank {
            accum.hit5 += 1;
            accum.mrr5 += 1.0 / rank as f32;
        }
        let user = accum.per_user.entry(example.user_id.clone()).or_default();
        user.examples += 1;
        if rank == Some(1) {
            user.hit1 += 1;
        }
    }
    accum.into_report()
}

impl MetricsAccum {
    fn into_report(self) -> MetricsReport {
        let denom = self.examples.max(1) as f32;
        let macro_hit_rate_at_1_pct = if self.per_user.is_empty() {
            0.0
        } else {
            self.per_user
                .values()
                .map(|user| user.hit1 as f32 / user.examples.max(1) as f32)
                .sum::<f32>()
                / self.per_user.len() as f32
                * 100.0
        };
        MetricsReport {
            examples: self.examples,
            predicted: self.predicted,
            hit_rate_at_1_pct: pct(self.hit1, denom),
            hit_rate_at_3_pct: pct(self.hit3, denom),
            hit_rate_at_5_pct: pct(self.hit5, denom),
            mean_reciprocal_rank_at_5: round3(self.mrr5 / denom),
            prediction_coverage_pct: pct(self.predicted, denom),
            macro_hit_rate_at_1_pct: round3(macro_hit_rate_at_1_pct),
        }
    }
}

fn pct(numerator: usize, denom: f32) -> f32 {
    round3(numerator as f32 / denom * 100.0)
}

fn round3(value: f32) -> f32 {
    (value * 1000.0).round() / 1000.0
}

fn hour_bucket(timestamp_ms: i64) -> u8 {
    let seconds = timestamp_ms.div_euclid(1000);
    ((seconds.div_euclid(3600)).rem_euclid(24)) as u8
}

fn weekday(timestamp_ms: i64) -> u8 {
    let days = timestamp_ms.div_euclid(86_400_000);
    ((days + 4).rem_euclid(7)) as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn train_and_eval_lsapp_fixture() {
        let dir = unique_tmp_dir();
        fs::create_dir_all(&dir).expect("create tmp dir");
        let input = dir.join("lsapp.csv");
        let artifact = dir.join("artifact.json");
        let report = dir.join("report.json");
        write_fixture(&input);

        train(TrainOptions {
            dataset: NextAppDataset::Lsapp,
            input: input.clone(),
            output: artifact.clone(),
            horizon_secs: 30,
            history_len: 3,
            split: NextAppSplit::Standard,
        })
        .expect("train should succeed");
        evaluate(EvalOptions {
            dataset: NextAppDataset::Lsapp,
            input,
            artifact,
            output: report.clone(),
            horizon_secs: 30,
            history_len: 3,
            split: NextAppSplit::Standard,
        })
        .expect("eval should succeed");

        let value: serde_json::Value =
            serde_json::from_reader(File::open(report).expect("report should exist"))
                .expect("report should parse");
        assert_eq!(value["schema_version"], "dipecs.next_app_eval.v1");
        assert!(value["metrics"]["ensemble"]["examples"].as_u64().unwrap() > 0);
    }

    fn write_fixture(path: &Path) {
        let mut file = File::create(path).expect("create fixture");
        writeln!(file, "user_id,session_id,timestamp_ms,app_name,event_type").unwrap();
        for user in ["u1", "u2"] {
            let mut ts = 1_000_i64;
            for session in ["s1", "s2", "s3"] {
                for app in [
                    "com.chat",
                    "com.mail",
                    "com.chat",
                    "com.mail",
                    "com.browser",
                ] {
                    writeln!(file, "{user},{session},{ts},{app},foreground").unwrap();
                    ts += 5_000;
                }
                ts += 60_000;
            }
        }
    }

    fn unique_tmp_dir() -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("dipecs-next-app-test-{suffix}"))
    }
}
