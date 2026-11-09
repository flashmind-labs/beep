//! Read-only period summaries of retained local metrics.

use serde::{Deserialize, Serialize};

pub const PROTOCOL: &str = "metrics/1";
pub const MAX_PERIOD: u64 = 7 * 24 * 60 * 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub since: u64,
    pub until: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Reply {
    Summary { text: String },
    Failed { reason: String },
}
