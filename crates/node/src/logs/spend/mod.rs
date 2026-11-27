//! What the agents' logs say sessions spent: one reader for each agent's
//! log (`scan`), the prices (`billing`) and the ledger of what was read.

use crate::logs::prices::Tokens;
use beep_proto::{SessionId, node::usage::Billing};

pub(crate) mod agy;
pub(crate) mod billing;
mod claude;
mod codex;
pub(crate) mod databases;
pub(crate) mod ledger;
mod pi;
pub(crate) mod scan;

pub(crate) use pi::session_cost as pi_session_cost;
pub(crate) use scan::Roots;

/// What one log line said.
#[derive(Debug)]
pub(crate) enum Line {
    /// The session the log is of, from its header, and the provider it
    /// runs on when the header names one.
    Session {
        id: SessionId,
        cwd: String,
        provider: Option<String>,
    },
    /// The model the next requests run on.
    Model(String),
    /// One request to the model.
    Request(Request),
}

/// One request and its tokens, as one log line records it.
#[derive(Debug, Default)]
pub(crate) struct Request {
    /// The session, when the line names it rather than the log's header.
    session: Option<SessionId>,
    cwd: Option<String>,
    /// Unix seconds.
    at: u64,
    model: Option<String>,
    tokens: Tokens,
    /// What it cost, when the log says.
    cost_usd: Option<f64>,
    /// Lines with the same key are copies of one request.
    key: Option<u64>,
    /// A running total that a repeated count repeats.
    total: Option<u64>,
    /// The provider it went to, when the line names it.
    provider: Option<String>,
    /// How it was paid for, when the line itself tells.
    billing: Billing,
}

