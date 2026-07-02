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
