//! Train/test split strategies for next-app evaluation.

use std::collections::BTreeSet;

use aios_agent::NextAppTrainingExample;

use super::NextAppSplit;

pub(crate) fn split_examples(
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
    let mut by_user: std::collections::BTreeMap<&str, Vec<&NextAppTrainingExample>> =
        std::collections::BTreeMap::new();
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
    let mut ranked_users: Vec<&str> = users.into_iter().collect();
    ranked_users.sort_by(|a, b| {
        stable_user_split_hash(a)
            .cmp(&stable_user_split_hash(b))
            .then_with(|| a.cmp(b))
    });
    let train_users: BTreeSet<&str> = ranked_users.into_iter().take(cutoff.max(1)).collect();
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

fn stable_user_split_hash(user_id: &str) -> u64 {
    const FNV_OFFSET: u64 = 14_695_981_039_346_656_037;
    const FNV_PRIME: u64 = 1_099_511_628_211;
    const SEED: &[u8] = b"dipecs-next-app-cold-start-v1";

    let mut hash = FNV_OFFSET;
    for byte in SEED.iter().chain([0].iter()).chain(user_id.as_bytes()) {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}
