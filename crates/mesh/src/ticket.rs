//! The tickets a desktop shares to bring a device into a mesh.

use crate::DeviceKind;
use beep_proto::lenient;
use iroh::{EndpointAddr, EndpointId, SecretKey};
use iroh_tickets::{ParseError, Ticket};
use serde::{Deserialize, Serialize};
use std::fmt::{self, Debug, Formatter};
use std::str::FromStr;
use subtle::ConstantTimeEq;
use thiserror::Error;

/// The prefix of tickets from before `beep-join/2`.
const OLD_KIND: &str = "beepjoin";

/// Random bytes in a ticket. `Debug` prints none of them.
#[derive(Clone, Serialize, Deserialize)]
pub struct JoinSecret(#[serde(with = "serde_bytes")] [u8; 32]);

impl JoinSecret {
    pub fn generate() -> Self {
        Self(SecretKey::generate().to_bytes())
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Compares in constant time.
    pub fn verify(&self, other: &JoinSecret) -> bool {
        self.0.ct_eq(&other.0).into()
    }
}

impl From<[u8; 32]> for JoinSecret {
    fn from(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

impl Debug for JoinSecret {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("JoinSecret(..)")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JoinTicket {
    #[serde(with = "crate::wire::id")]
    pub mesh: EndpointId,
    #[serde(with = "crate::wire::addr")]
    pub addr: EndpointAddr,
    pub kind: DeviceKind,
    pub secret: JoinSecret,
    pub expires_ms: u64,
    /// The device is temporary, such as a container: the desktop that spends the ticket writes
    /// `temporary/<id>` for it.
    #[serde(default)]
    pub temporary: bool,
    /// The joining phone observes hosts rather than hosting code itself.
    #[serde(default)]
    pub observer: bool,
}

/// The ticket desktops share: the desktop that minted the invite and its secret, short enough
/// for a QR code a phone reads off a screen. iroh finds the desktop by its id; the desktop keeps
/// the invite's kind, flags and expiry and enforces them when the secret is spent.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ShortTicket {
    #[serde(with = "crate::wire::id")]
    pub desktop: EndpointId,
    pub secret: JoinSecret,
}

impl ShortTicket {
    /// Reads the code in either case: the QR code carries it in capitals, which it packs smaller.
    pub fn parse(text: &str) -> Result<Self, TicketError> {
        Self::decode_string(&text.trim().to_ascii_lowercase()).map_err(|_| TicketError::Unreadable)
    }
}

impl Ticket for ShortTicket {
    const KIND: &'static str = "beepjoin3";

    fn encode_bytes(&self) -> Vec<u8> {
        rmp_serde::to_vec(self).expect("msgpack encodes a short ticket")
    }

    fn decode_bytes(bytes: &[u8]) -> Result<Self, ParseError> {
        lenient::from_slice(bytes)
            .map_err(|_| ParseError::verification_failed("the ticket cannot be read"))
    }
}

impl From<&JoinTicket> for ShortTicket {
    fn from(ticket: &JoinTicket) -> Self {
        Self {
            desktop: ticket.addr.id,
            secret: ticket.secret.clone(),
        }
    }
}

/// What a joiner redeems: where the desktop is, the secret, and the mesh it expects when the
/// ticket names one.
pub struct Invitation {
    pub(crate) addr: EndpointAddr,
    pub(crate) secret: JoinSecret,
    pub(crate) mesh: Option<EndpointId>,
}

impl From<&JoinTicket> for Invitation {
    fn from(ticket: &JoinTicket) -> Self {
        Self {
            addr: ticket.addr.clone(),
            secret: ticket.secret.clone(),
            mesh: Some(ticket.mesh),
        }
    }
}

impl From<&ShortTicket> for Invitation {
    fn from(ticket: &ShortTicket) -> Self {
        Self {
            addr: EndpointAddr::new(ticket.desktop),
            secret: ticket.secret.clone(),
            mesh: None,
        }
    }
}

impl FromStr for Invitation {
    type Err = TicketError;

    /// Reads a short ticket, or a full `beepjoin2` one from a released desktop.
    fn from_str(text: &str) -> Result<Self, TicketError> {
        if let Ok(code) = ShortTicket::parse(text) {
            return Ok(Into::<Invitation>::into(&code));
        }

        JoinTicket::parse(text).map(|ticket| Into::<Invitation>::into(&ticket))
    }
}

impl Invitation {
    pub(crate) fn addr(&self) -> &EndpointAddr {
        &self.addr
    }

    pub(crate) fn secret(&self) -> &JoinSecret {
        &self.secret
    }
}

/// Why ticket text could not be read. Neither case quotes the text.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum TicketError {
    /// A ticket from before `beep-join/2`; the desktop mints a new one.
    #[error("this ticket is from an older version; ask for a new one")]
    Old,
    #[error("that is not a join ticket")]
    Unreadable,
}

impl JoinTicket {
    /// Reads the text a desktop shows. Use this rather than `decode_string`,
    /// which cannot tell an old ticket from a mistyped one.
    pub fn parse(text: &str) -> Result<Self, TicketError> {
        let text = text.trim();
        // Base32 includes `2`, so an old ticket can begin like a new one:
        // decide only after the new format failed to read it.
        Self::decode_string(text).map_err(|_| match text.starts_with(OLD_KIND) {
            true => TicketError::Old,
            false => TicketError::Unreadable,
        })
    }

    pub fn expired(&self, now_ms: u64) -> bool {
        now_ms >= self.expires_ms
    }
}

/// Deprecated: desktops share a `ShortTicket` and only read this form, from released desktops.
impl Ticket for JoinTicket {
    const KIND: &'static str = "beepjoin2";

    fn encode_bytes(&self) -> Vec<u8> {
        // Every field is plain data that msgpack always encodes.
        rmp_serde::to_vec(self).expect("msgpack encodes a ticket")
    }

    fn decode_bytes(bytes: &[u8]) -> Result<Self, ParseError> {
        lenient::from_slice(bytes)
            .map_err(|_| ParseError::verification_failed("the ticket cannot be read"))
    }
}
