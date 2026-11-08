//! Mobile access to existing workspace requests. No wire types live here.

mod agents;
mod instructions;
mod projects;
mod terminals;
mod usage;

pub(crate) use agents::CodingStart;
pub(crate) use terminals::{
    CommandStart, TerminalConnection, TerminalInputStream, TerminalOutputStream,
};

use crate::BeepError;
use uuid::Uuid;

fn parse_id(id: &str) -> Result<Uuid, BeepError> {
    Uuid::parse_str(id).map_err(|_| BeepError::BadRequest {
        reason: "Invalid workspace identifier".into(),
    })
}
