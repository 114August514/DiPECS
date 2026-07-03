//! LSApp-shaped dataset loading and example construction.

use std::collections::{BTreeMap, VecDeque};
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use aios_agent::NextAppTrainingExample;
use anyhow::{bail, Context, Result};

#[derive(Debug, Clone)]
pub(crate) struct LsAppRecord {
    pub user_id: String,
    pub session_id: String,
    pub timestamp_ms: i64,
    pub app_name: String,
    pub event_type: String,
    pub ordinal: usize,
}

pub(crate) fn load_examples(
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

fn hour_bucket(timestamp_ms: i64) -> u8 {
    let seconds = timestamp_ms.div_euclid(1000);
    ((seconds.div_euclid(3600)).rem_euclid(24)) as u8
}

fn weekday(timestamp_ms: i64) -> u8 {
    let days = timestamp_ms.div_euclid(86_400_000);
    ((days + 4).rem_euclid(7)) as u8
}
