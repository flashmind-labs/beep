//! Linking a second device to an account, either way round.
//!
//! A computer with no account yet makes its own key and shows a `LinkOffer`
//! as a QR code. The offer is signed with that key and names the server, so
//! the phone can only hand the server an offer the device made for it. The
//! phone sends it with `LinkDevice`, and from then on the device's key signs
//! into the phone's account.
//!
//! A device that already has an account, such as a computer someone started
//! on, shows a join code instead (`InviteDevice`). The phone scans or types
//! it and sends `JoinAccount`, which moves the phone's own account into the
//! computer's.
//!
//! A server that runs for one person shows its own offer when it starts: a
//! `LinkOffer` it signed for itself, naming its own key as both device and
//! server. An app that scans one learns which server to connect to (see
//! `LinkOffer::server`); the first account to connect owns that server.

pub use crate::format_group_code as format_join_code;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use iroh::{EndpointId, SecretKey, Signature};
use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// How long a phone may take to scan an offer.
pub const OFFER_TTL: Duration = Duration::from_secs(10 * 60);
/// The longest device name an offer may carry, in bytes.
pub const MAX_NAME: usize = 64;

const CONTEXT: &[u8] = b"beep-link/1";
/// Crockford's base32, which leaves out letters that look like digits.
const CODE_ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const SCHEME: &str = "beep-link:";
const JOIN_SCHEME: &str = "beep-join:";

/// A device asking to join the account of whoever scans it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LinkOffer {
    pub device: EndpointId,
    /// What the device calls itself, such as "MacBook".
    pub name: String,
    /// Seconds since the Unix epoch.
    pub signed_at: u64,
    pub signature: Signature,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum LinkError {
    #[error("not a link code")]
    Malformed,
    #[error("the device name is empty or too long")]
    BadName,
    #[error("the link code is for another server")]
    WrongServer,
    #[error("the link code expired; show a new one")]
    Expired,
    #[error("the link code's signature does not match")]
    BadSignature,
}

impl LinkOffer {
    pub fn sign(
        identity: &SecretKey,
        server: EndpointId,
        name: &str,
        now: SystemTime,
    ) -> Result<Self, LinkError> {
        check_name(name)?;
        let signed_at = unix_secs(now);
        let device = identity.public();
        let signature = identity.sign(&message(server, device, signed_at, name));
        Ok(Self {
            device,
            name: name.to_owned(),
            signed_at,
            signature,
        })
    }

    /// Checks that the device made this offer for `server` within `OFFER_TTL`
    /// of `now`.
    pub fn verify(&self, server: EndpointId, now: SystemTime) -> Result<(), LinkError> {
        check_name(&self.name)?;
        self.device
            .verify(
                &message(server, self.device, self.signed_at, &self.name),
                &self.signature,
            )
            .map_err(|_| LinkError::BadSignature)?;
        let now = unix_secs(now);
        // A minute of clock skew either way.
        let fresh = self.signed_at <= now + 60 && now <= self.signed_at + OFFER_TTL.as_secs();
        if !fresh {
            return Err(LinkError::Expired);
        }
        Ok(())
    }

    /// A server's offer of itself, for an app that has no server yet.
    pub fn for_server(
        identity: &SecretKey,
        name: &str,
        now: SystemTime,
    ) -> Result<Self, LinkError> {
        Self::sign(identity, identity.public(), name, now)
    }

    /// The server to connect to, when this is a server's offer of itself
    /// (see `for_server`) signed within `OFFER_TTL` of `now`. `None` for a
    /// device's offer, which only verifies for the server it was made for.
    pub fn server(&self, now: SystemTime) -> Option<EndpointId> {
        self.verify(self.device, now).ok().map(|()| self.device)
    }

    /// Six characters both screens show, such as `7K4 PQ2`, so the user can
    /// check the phone scanned the right computer.
    pub fn code(&self) -> String {
        let hash = blake3::hash(&self.signature.to_bytes());
        let chars: Vec<char> = hash.as_bytes()[..6]
            .iter()
            .map(|byte| char::from(CODE_ALPHABET[usize::from(byte % 32)]))
            .collect();
        let (first, second) = chars.split_at(3);
        format!(
            "{} {}",
            first.iter().collect::<String>(),
            second.iter().collect::<String>()
        )
    }

    /// When the offer stops being accepted.
    pub fn expires_at(&self) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(self.signed_at) + OFFER_TTL
    }

    /// The offer as text for a QR code.
    pub fn to_uri(&self) -> String {
        // Serializing plain fields into a Vec cannot fail.
        let body = rmp_serde::to_vec(self).unwrap_or_default();
        format!("{SCHEME}{}", URL_SAFE_NO_PAD.encode(body))
    }

    pub fn parse(uri: &str) -> Result<Self, LinkError> {
        let body = uri
            .trim()
            .strip_prefix(SCHEME)
            .ok_or(LinkError::Malformed)?;
        let bytes = URL_SAFE_NO_PAD
            .decode(body)
            .map_err(|_| LinkError::Malformed)?;
        rmp_serde::from_slice(&bytes).map_err(|_| LinkError::Malformed)
    }
}

/// A join code as text for a QR code.
pub fn join_uri(code: &str) -> String {
    format!("{JOIN_SCHEME}{code}")
}

/// The join code in what someone scanned, typed or pasted: a `beep-join:`
/// QR code, or the code itself with any case, spaces or dashes. None for
/// other text, such as a link offer.
pub fn join_code(input: &str) -> Option<String> {
    let input = input.trim();
    let code = crate::urls::plain_code(input.strip_prefix(JOIN_SCHEME).unwrap_or(input));
    let valid = code.len() == 10 && code.bytes().all(|b| b.is_ascii_alphanumeric());
    valid.then_some(code)
}

fn check_name(name: &str) -> Result<(), LinkError> {
    if name.trim().is_empty() || name.len() > MAX_NAME {
        return Err(LinkError::BadName);
    }
    Ok(())
}

fn unix_secs(at: SystemTime) -> u64 {
    at.duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn message(server: EndpointId, device: EndpointId, signed_at: u64, name: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(CONTEXT.len() + 72 + name.len());
    out.extend_from_slice(CONTEXT);
    out.extend_from_slice(server.as_bytes());
    out.extend_from_slice(device.as_bytes());
    out.extend_from_slice(&signed_at.to_be_bytes());
    out.extend_from_slice(name.as_bytes());
    out
}
