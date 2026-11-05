//! The ticket a desktop mints to bring a device in. The secret proves the
//! holder was given the ticket; the desktop checks it on `beep-join/2`.

use crate::DeviceKind;
use beep_proto::lenient;
use iroh::{EndpointAddr, EndpointId, SecretKey};
use iroh_tickets::{ParseError, Ticket};
use serde::{Deserialize, Serialize};
use std::fmt::{self, Debug, Formatter};
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

    pub fn expose(&self) -> &[u8; 32] {
        &self.0
    }

    /// Compares in constant time.
    pub fn verify(&self, other: &JoinSecret) -> bool {
        self.0.ct_eq(&other.0).into()
    }
}

impl Debug for JoinSecret {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("JoinSecret(..)")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JoinTicket {
    #[serde(with = "crate::bin::id")]
    pub mesh: EndpointId,
    #[serde(with = "crate::bin::addr")]
    pub addr: EndpointAddr,
    pub kind: DeviceKind,
    pub secret: JoinSecret,
    pub expires_ms: u64,
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
        let old = text.starts_with(OLD_KIND) && !text.starts_with(Self::KIND);
        if old {
            return Err(TicketError::Old);
        }
        Self::decode_string(text).map_err(|_| TicketError::Unreadable)
    }

    pub fn expired(&self, now_ms: u64) -> bool {
        now_ms >= self.expires_ms
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DeviceKind;
    use iroh::{EndpointAddr, SecretKey};

    fn ticket() -> JoinTicket {
        let key = SecretKey::generate();
        JoinTicket {
            mesh: key.public(),
            addr: EndpointAddr::new(key.public()),
            kind: DeviceKind::Node,
            secret: JoinSecret::generate(),
            expires_ms: 1_700_000_000_000,
        }
    }

    #[test]
    fn a_ticket_reads_back_from_its_text() {
        let t = ticket();
        let text = t.encode_string();
        assert!(text.starts_with("beepjoin"));
        let back = JoinTicket::decode_string(&text).unwrap();
        assert_eq!(back.mesh, t.mesh);
        assert_eq!(back.addr, t.addr);
        assert_eq!(back.kind, t.kind);
        assert_eq!(back.expires_ms, t.expires_ms);
        assert!(back.secret.verify(&t.secret));
    }

    #[test]
    fn an_old_ticket_says_so() {
        // Postcard-era tickets are `beepjoin` followed by base32.
        assert_eq!(
            JoinTicket::parse("beepjoinabcdefghijklmnop").unwrap_err(),
            TicketError::Old
        );
        assert_eq!(
            JoinTicket::parse("beepjoin2abcdefg").unwrap_err(),
            TicketError::Unreadable
        );
        assert_eq!(
            JoinTicket::parse("hello").unwrap_err(),
            TicketError::Unreadable
        );
        let text = ticket().encode_string();
        assert!(JoinTicket::parse(&format!(" {text}\n")).is_ok());
    }

    #[test]
    fn a_secret_verifies_only_itself() {
        let (a, b) = (JoinSecret::generate(), JoinSecret::generate());
        assert!(a.verify(&a.clone()));
        assert!(!a.verify(&b));
    }

    #[test]
    fn a_ticket_expires_at_its_deadline() {
        let t = ticket();
        assert!(!t.expired(t.expires_ms - 1));
        assert!(t.expired(t.expires_ms));
    }

    #[test]
    fn a_tickets_debug_hides_the_secret() {
        let t = ticket();
        let secret = t.secret.expose();
        let hex: String = secret.iter().map(|b| format!("{b:02x}")).collect();
        let shown = format!("{t:?} {:?}", t.secret);
        assert!(!shown.contains(&hex));
        assert!(!shown.contains(&format!("{secret:?}")));
    }
}
