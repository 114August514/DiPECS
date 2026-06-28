//! # aios-spec — DiPECS 宪法层
//!
//! 零内部依赖。定义全系统的核心数据结构、Trait 和 IPC 协议。
//! 所有跨模块通信必须依赖此层的抽象。

#![deny(unsafe_op_in_unsafe_fn)]

pub mod context;
pub mod event;
pub mod governance;
pub mod intent;
pub mod sanitized;
pub mod trace;

pub use context::*;
pub use event::*;
pub use governance::*;
pub use intent::*;
pub use sanitized::*;
pub use trace::*;

/// aios-spec 定义的公共 trait
pub mod traits {
    mod privacy;
    mod trace_validator;

    pub use privacy::PrivacySanitizer;
    pub use trace_validator::TraceValidator;
}
