//! Setup tokens: how a machine links itself to an account.
//!
//! The signed-in user asks the server for one (`CreateNodeToken`) and runs
//! `beep-node setup <token>` on the machine. The token names the server and
//! carries a random secret, so the machine needs nothing else: it connects
//! to that server and hands the token over in its `NodeHello`, and the server
//! links the machine's key to the user who asked. The server keeps only
//! `SetupToken::hash`, and a token works once.
//!
//! As text a token is `bk1-` and 80 characters of Crockford's base32 in
//! groups of five: the server's key, the secret and a two-byte checksum, so
//! a token cut short or mistyped is caught before anything connects.

use iroh::{EndpointId, SecretKey};
use std::{fmt, time::Duration};

/// How long a token works.
pub const TOKEN_TTL: Duration = Duration::from_secs(10 * 60);

const PREFIX: &str = "bk1-";
const SECRET_LEN: usize = 16;
const CHECK_LEN: usize = 2;
const BODY_LEN: usize = 32 + SECRET_LEN + CHECK_LEN;
/// 400 bits, five to a character.
const TEXT_LEN: usize = BODY_LEN * 8 / 5;
const GROUP: usize = 5;
/// Crockford's base32, which leaves out letters that look like digits.
const ALPHABET: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";
const HASH_CONTEXT: &str = "beep node setup token 2026-11 secret hash";

/// A one-time token that links a machine to the account that made it.
#[derive(Clone, PartialEq, Eq)]
pub struct SetupToken {
    /// The server that made it, which the machine connects to.
    pub server: EndpointId,
    secret: [u8; SECRET_LEN],
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TokenError {
    #[error("this is not a setup token; it starts with `bk1-`")]
    Malformed,
    #[error("this setup token is mistyped or cut short; copy it again")]
    BadCheck,
    #[error("the setup token names no valid server")]
    BadServer,
}

impl SetupToken {
    /// A new token for `server` with a fresh secret.
    pub fn generate(server: EndpointId) -> Self {
        // A new key's bytes come from the system's secure random source,
        // the same one every Beep key is made from.
        let bytes = SecretKey::generate().to_bytes();
        let mut secret = [0; SECRET_LEN];
        secret.copy_from_slice(&bytes[..SECRET_LEN]);
        Self { server, secret }
    }

    /// What the server stores in place of the secret, as hex.
    pub fn hash(&self) -> String {
        blake3::derive_key(HASH_CONTEXT, &self.secret)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    /// Reads a token as `Display` writes it, in any case, with or without
    /// its dashes, and with spaces or line breaks a paste added.
    pub fn parse(text: &str) -> Result<Self, TokenError> {
        let text = text.trim();
        let prefix = text.get(..PREFIX.len()).ok_or(TokenError::Malformed)?;
        if !prefix.eq_ignore_ascii_case(PREFIX) {
            return Err(TokenError::Malformed);
        }
        let digits: Vec<u8> = text[PREFIX.len()..]
            .chars()
            .filter(|c| !c.is_whitespace() && *c != '-')
            .map(digit)
            .collect::<Option<_>>()
            .ok_or(TokenError::Malformed)?;
        if digits.len() != TEXT_LEN {
            return Err(TokenError::Malformed);
        }
        let body = decode(&digits);
        let (key, rest) = body.split_at(32);
        let (secret, check) = rest.split_at(SECRET_LEN);
        if check != checksum(key, secret) {
            return Err(TokenError::BadCheck);
        }
        let key: [u8; 32] = key.try_into().map_err(|_| TokenError::Malformed)?;
        let server = EndpointId::from_bytes(&key).map_err(|_| TokenError::BadServer)?;
        let secret = secret.try_into().map_err(|_| TokenError::Malformed)?;
        Ok(Self { server, secret })
    }

    /// The command that sets a machine up with this token.
    pub fn command(&self) -> String {
        setup_command(&self.to_string())
    }
}

/// `beep-node setup <token>`, for showing next to a token as
/// `CreateNodeToken` gave it.
pub fn setup_command(token: &str) -> String {
    format!("beep-node setup {}", token.trim())
}

impl fmt::Display for SetupToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut body = Vec::with_capacity(BODY_LEN);
        body.extend_from_slice(self.server.as_bytes());
        body.extend_from_slice(&self.secret);
        body.extend_from_slice(&checksum(self.server.as_bytes(), &self.secret));
        f.write_str(PREFIX)?;
        for (i, chunk) in encode(&body).chunks(GROUP).enumerate() {
            if i > 0 {
                f.write_str("-")?;
            }
            f.write_str(std::str::from_utf8(chunk).map_err(|_| fmt::Error)?)?;
        }
        Ok(())
    }
}

/// Never shows the secret.
impl fmt::Debug for SetupToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SetupToken")
            .field("server", &self.server)
            .finish_non_exhaustive()
    }
}

fn checksum(key: &[u8], secret: &[u8]) -> [u8; CHECK_LEN] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(key);
    hasher.update(secret);
    let hash = hasher.finalize();
    [hash.as_bytes()[0], hash.as_bytes()[1]]
}

/// The value of one base32 character, reading the letters people mix up
/// with digits as those digits.
fn digit(c: char) -> Option<u8> {
    let c = match c.to_ascii_lowercase() {
        'o' => '0',
        'i' | 'l' => '1',
        c => c,
    };
    let byte = u8::try_from(c).ok()?;
    ALPHABET
        .iter()
        .position(|&a| a == byte)
        .and_then(|i| u8::try_from(i).ok())
}

/// `bytes` as base32 characters; its length in bits must divide by five.
fn encode(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() * 8 / 5);
    let mut buffer: u16 = 0;
    let mut bits = 0;
    for &byte in bytes {
        buffer = (buffer << 8) | u16::from(byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHABET[usize::from((buffer >> bits) & 31)]);
        }
        buffer &= (1 << bits) - 1;
    }
    out
}

/// Base32 digit values back to bytes; the count of digits must make whole
/// bytes.
fn decode(digits: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(digits.len() * 5 / 8);
    let mut buffer: u16 = 0;
    let mut bits = 0;
    for &d in digits {
        buffer = (buffer << 5) | u16::from(d);
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push(((buffer >> bits) & 0xff) as u8);
        }
        buffer &= (1 << bits) - 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server() -> EndpointId {
        SecretKey::from_bytes(&[4; 32]).public()
    }

    #[test]
    fn a_token_survives_its_text() {
        let token = SetupToken::generate(server());
        let text = token.to_string();
        assert!(text.starts_with("bk1-"), "{text}");
        assert_eq!(text.len(), 4 + TEXT_LEN + TEXT_LEN / GROUP - 1);
        assert_eq!(SetupToken::parse(&text).unwrap(), token);
        // Pasted in capitals, without dashes or with a line break.
        let rest = text[4..].to_uppercase().replacen('-', "", 5);
        let loose = format!(" BK1-{}\n", rest.replace('-', "\n"));
        assert_eq!(SetupToken::parse(&loose).unwrap(), token);
        assert_eq!(
            token.command(),
            format!("beep-node setup {text}"),
            "the command is the token's"
        );
    }

    #[test]
    fn two_tokens_differ_and_hash_apart() {
        let a = SetupToken::generate(server());
        let b = SetupToken::generate(server());
        assert_ne!(a, b);
        assert_ne!(a.hash(), b.hash());
        assert_eq!(a.hash().len(), 64);
        assert!(!a.hash().contains(&a.to_string()[4..]));
        assert!(!format!("{a:?}").contains(&a.to_string()[60..]));
    }

    #[test]
    fn a_mistyped_token_is_caught() {
        let text = SetupToken::generate(server()).to_string();
        let mut chars: Vec<char> = text.chars().collect();
        let i = 20;
        chars[i] = if chars[i] == 'a' { 'b' } else { 'a' };
        let typo: String = chars.into_iter().collect();
        assert_eq!(SetupToken::parse(&typo), Err(TokenError::BadCheck));
        assert_eq!(
            SetupToken::parse(&text[..text.len() - 1]),
            Err(TokenError::Malformed)
        );
        assert_eq!(
            SetupToken::parse("beep-link:abc"),
            Err(TokenError::Malformed)
        );
        assert_eq!(SetupToken::parse("bk1-u!"), Err(TokenError::Malformed));
        assert_eq!(SetupToken::parse("bk"), Err(TokenError::Malformed));
    }

    #[test]
    fn base32_round_trips() {
        let bytes: Vec<u8> = (0..BODY_LEN as u8).collect();
        let text = encode(&bytes);
        assert_eq!(text.len(), TEXT_LEN);
        let digits: Vec<u8> = text
            .iter()
            .map(|&c| digit(char::from(c)).unwrap())
            .collect();
        assert_eq!(decode(&digits), bytes);
    }
}
