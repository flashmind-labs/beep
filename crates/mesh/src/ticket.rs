//! The ticket a desktop mints to bring a device in. The secret proves the
//! holder was given the ticket; the desktop checks it on `beep-join/1`.

use crate::DeviceKind;
use iroh::{EndpointAddr, EndpointId, SecretKey};
use iroh_tickets::{ParseError, Ticket};
use serde::{Deserialize, Serialize};
use std::fmt::{self, Debug, Formatter};

/// Random bytes in a ticket. `Debug` prints none of them.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JoinSecret([u8; 32]);

impl JoinSecret {
    pub fn generate() -> Self {
        Self(SecretKey::generate().to_bytes())
    }

    pub fn expose(&self) -> &[u8; 32] {
        &self.0
    }
}

impl Debug for JoinSecret {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("JoinSecret(..)")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JoinTicket {
    pub mesh: EndpointId,
    pub addr: EndpointAddr,
    pub kind: DeviceKind,
    pub secret: JoinSecret,
    pub expires_ms: u64,
}

impl Ticket for JoinTicket {
    const KIND: &'static str = "beepjoin";

    fn encode_bytes(&self) -> Vec<u8> {
        // Every field is plain data that postcard always encodes, as in iroh's own tickets.
        postcard::to_stdvec(self).expect("postcard encodes a ticket")
    }

    fn decode_bytes(bytes: &[u8]) -> Result<Self, ParseError> {
        Ok(postcard::from_bytes(bytes)?)
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
        assert_eq!(JoinTicket::decode_string(&text).unwrap(), t);
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
