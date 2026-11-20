//! Team tickets: an invite to a forge repository's team, carried to a person in
//! another mesh. The inviter's desktop keeps the invite's expiry and enforces it
//! when the secret is spent.

use crate::{JoinSecret, TicketError};
use beep_proto::{lenient, team::TeamRepo};
use iroh::EndpointId;
use iroh_tickets::{ParseError, Ticket};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TeamTicket {
    #[serde(with = "crate::wire::id")]
    pub inviter: EndpointId,
    pub repo: TeamRepo,
    pub secret: JoinSecret,
}

impl TeamTicket {
    /// Reads the code in either case: a QR code carries it in capitals.
    pub fn parse(text: &str) -> Result<Self, TicketError> {
        Self::decode_string(&text.trim().to_ascii_lowercase()).map_err(|_| TicketError::Unreadable)
    }
}

impl Ticket for TeamTicket {
    const KIND: &'static str = "beepteam1";

    fn encode_bytes(&self) -> Vec<u8> {
        rmp_serde::to_vec(self).expect("msgpack encodes a team ticket")
    }

    fn decode_bytes(bytes: &[u8]) -> Result<Self, ParseError> {
        lenient::from_slice(bytes)
            .map_err(|_| ParseError::verification_failed("the ticket cannot be read"))
    }
}

/// What the inviter stores in place of a spent invite's secret.
pub fn secret_hash(secret: &JoinSecret) -> [u8; 32] {
    *blake3::hash(secret.as_bytes()).as_bytes()
}

#[cfg(test)]
#[path = "tests/team.rs"]
mod tests;
