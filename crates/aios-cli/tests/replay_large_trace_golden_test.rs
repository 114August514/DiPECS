//! 大 trace replay 的 audit hash 稳定性回归测试。
//!
//! `data/traces/android_synthetic_large.redacted.jsonl` 是 2400 行、1631 条有效事件的
//! 合成 Android trace。本测试把它的 canonical audit hash 钉死,防止隐私/聚合/决策/
//! 执行各阶段的输出在重构中悄悄漂移。

use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

use aios_cli::replay::{self, Stage};

/// 钉死的 canonical audit hash。
/// 若此处失败,先检查改动是否是语义变更;若是,用测试输出更新此常量。
const LARGE_TRACE_GOLDEN_HASH: &str =
    "sha256:2b3c5ac19314ac5128910fd26db3e02e76291cd495ee6fe87552a2b26ea7cde2";

fn large_trace_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../data/traces/android_synthetic_large.redacted.jsonl")
}

#[test]
fn large_trace_audit_hash_is_stable() {
    let path = large_trace_path();
    let file = File::open(&path).expect("large trace must exist");
    let reader = BufReader::new(file);

    let mut ndjson_sink: Vec<u8> = Vec::new();
    let mut audit_sink: Vec<u8> = Vec::new();

    let outcome = replay::run_with_audit(
        reader,
        &mut ndjson_sink,
        &mut audit_sink,
        60,
        Stage::Execute,
    )
    .expect("replay should succeed");

    assert!(
        outcome.audit_hash.starts_with("sha256:") && outcome.audit_hash.len() == 71,
        "canonical replay hash must be a sha256 digest, got {}",
        outcome.audit_hash
    );

    // 先跑一遍拿到真实 hash,再固化到 LARGE_TRACE_GOLDEN_HASH。
    assert_eq!(
        outcome.audit_hash, LARGE_TRACE_GOLDEN_HASH,
        "large trace audit hash drifted. expected: {LARGE_TRACE_GOLDEN_HASH}, got: {}",
        outcome.audit_hash
    );

    assert_eq!(outcome.summary.events_ingested, 1631);
    assert_eq!(outcome.summary.audit_hash, LARGE_TRACE_GOLDEN_HASH);
}
