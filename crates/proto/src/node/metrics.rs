//! Read-only period summaries of retained local metrics.

use serde::{Deserialize, Serialize};

pub const PROTOCOL: &str = "metrics/1";
pub const QUERY_PROTOCOL: &str = "metrics/2";
pub const MAX_PERIOD: u64 = 7 * 24 * 60 * 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub since: u64,
    pub until: u64,
}

/// Filtered, paginated summaries. `metrics/1` retains its original request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Query {
    /// UTC RFC 3339 bounds, start inclusive and end exclusive.
    pub start: String,
    pub end: String,
    pub search: Option<String>,
    pub regex: bool,
    pub limit: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Reply {
    Summary { text: String },
    Failed { reason: String },
}
