//! Simple baseline predictors for the next-app benchmark.

mod context;
mod frequency;
mod notification;
mod simple;

pub use context::{LastAppPrewarmBackend, LastForegroundBackend};
pub use frequency::{GlobalMajorityBackend, MarkovBackend, PerCurrentAppMajorityBackend};
pub use notification::{NotificationPriorityBackend, RecentNotificationBackend};
pub use simple::{AlwaysNoOpBackend, FirstCandidateBackend, RandomCandidateBackend};
