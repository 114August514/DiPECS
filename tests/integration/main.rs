//! DiPECS baseline integration tests.
//!
//! Run all baselines: `cargo test --test integration`
//! Run a single baseline: `cargo test --test integration policy_denial`

mod action_success_rate;
mod benchmark_cache;
mod cloud_llm_stability;
mod helpers;
mod noop_coverage;
mod policy_denial;
mod rationale_coverage;
mod routing_strategy;
mod signature_cross_verify;
mod window_size;
