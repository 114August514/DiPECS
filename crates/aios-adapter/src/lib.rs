//! # aios-adapter — 平台适配层
//!
//! 封装 Android/Linux 系统底层访问。
//! 提供:
//! - `/proc` 文件系统读取 (进程状态、内存、OOM 分数)
//! - Binder eBPF tracepoint 订阅 (跨进程通信监控)
//! - fanotify 文件系统监控
//! - 系统状态聚合 (电池、网络、位置)

#![deny(unsafe_op_in_unsafe_fn)]

pub mod binder_probe;
pub mod collection_stats;
pub mod daemon;
pub mod proc_reader;
pub mod system_collector;

pub use binder_probe::BinderProbe;
pub use proc_reader::ProcReader;
pub use system_collector::SystemStateCollector;
