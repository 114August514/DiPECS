//! # aios-core — DiPECS 核心引擎
//!
//! 职责: 事件聚合、隐私脱敏、策略校验、Trace 回放。
//! 内部逻辑保持同步 (Sync), 只在系统边界使用 async。

#![deny(unsafe_op_in_unsafe_fn)]

pub mod action_bus;
pub mod action_lifecycle;
pub mod collector_ingress;
pub mod context_builder;
pub mod governance;
pub mod policy_engine;
pub mod privacy_airgap;
pub mod text_analysis;
pub mod trace_engine;
