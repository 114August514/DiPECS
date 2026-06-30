//! # aios-core 鈥?DiPECS 鏍稿績寮曟搸
//!
//! 鑱岃矗: 浜嬩欢鑱氬悎銆侀殣绉佽劚鏁忋€佺瓥鐣ユ牎楠屻€乀race 鍥炴斁銆?//! 鍐呴儴閫昏緫淇濇寔鍚屾 (Sync), 鍙湪绯荤粺杈圭晫浣跨敤 async銆?
#![deny(unsafe_op_in_unsafe_fn)]

pub mod action_bus;
pub mod action_lifecycle;
pub mod collector_ingress;
pub mod context_builder;
pub mod context_memory;
pub mod governance;
pub mod policy_engine;
pub mod privacy_airgap;
pub mod text_analysis;
pub mod trace_engine;
