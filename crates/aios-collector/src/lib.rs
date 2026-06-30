//! # aios-collector - Rust collection layer
//!
//! Wraps Android/Linux system observation inputs and emits `RawEvent` values.
//! This crate exposes public-API Android JSONL ingress, `/proc` scanning,
//! Binder/eBPF and fanotify monitor interfaces, and system state snapshots.
//! Privileged monitors report explicit unavailable states when the current
//! deployment cannot attach to kernel facilities.
#![deny(unsafe_op_in_unsafe_fn)]

pub mod android_jsonl;
pub mod binder_probe;
pub mod collection_stats;
pub mod fanotify_monitor;
pub mod proc_reader;
pub mod system_collector;

pub use android_jsonl::{AndroidJsonlError, AndroidJsonlIngress, AndroidJsonlTailer};
pub use binder_probe::{BinderProbe, BinderProbeStatus};
pub use fanotify_monitor::{FanotifyConfig, FanotifyMonitor, FanotifyStatus};
pub use proc_reader::ProcReader;
pub use system_collector::SystemStateCollector;
