//! # aios-collector — Rust 采集层
//!
//! 封装 Android/Linux 系统观测入口, 输出 `RawEvent`。
//! 提供:
//! - `/proc` 文件系统读取 (进程状态、内存、OOM 分数)
//! - Binder eBPF tracepoint 订阅 (跨进程通信监控)
//! - 系统状态聚合 (电池、网络、位置)
//!
//! 文件系统访问事件的协议结构已在 `aios-spec` 中预留, 具体采集器尚未接入。

#![deny(unsafe_op_in_unsafe_fn)]

pub mod android_jsonl;
pub mod binder_probe;
pub mod collection_stats;
pub mod proc_reader;
pub mod system_collector;

pub use android_jsonl::{AndroidJsonlError, AndroidJsonlIngress, AndroidJsonlTailer};
pub use binder_probe::BinderProbe;
pub use proc_reader::ProcReader;
pub use system_collector::SystemStateCollector;
