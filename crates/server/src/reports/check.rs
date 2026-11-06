//! What a report must pass before it is kept: size, headers, clock,
//! signature and manifest, in that order.

use std::str::FromStr;

use axum::http::{HeaderMap, StatusCode};
use beep_proto::report::{
    KEY_HEADER, MAX_AGE, MAX_BODY, SIGNATURE_HEADER, TIME_HEADER, signed_message,
};
use beep_report::{Manifest, read_manifest};
use iroh::{EndpointId, Signature};
use thiserror::Error;

/// Why a report was refused; the text is what the sender sees.
#[derive(Debug, Error, PartialEq, Eq)]
pub(crate) enum Refused {
    #[error("The report is over {} MB.", MAX_BODY / (1024 * 1024))]
    TooLarge,
    #[error("The report did not arrive whole.")]
    Body,
    #[error("The report's {0} header is missing or does not read.")]
    Header(&'static str),
    #[error("This computer's clock is off by {0} minutes; set it right and send again.")]
    Clock(u64),
    #[error("The report's signature does not check out.")]
    Signature,
    #[error("The report's manifest does not read.")]
    Manifest,
    #[error("The report names another sender.")]
    Sender,
    #[error("refused")]
    Blocked,
    #[error("This computer has sent too many reports today; try again tomorrow.")]
    OverLimit,
    #[error("Problem reports are not set up on this server yet.")]
    NotSetUp,
}

impl Refused {
    pub(crate) fn status(&self) -> StatusCode {
        match self {
            Self::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::Body
            | Self::Header(_)
            | Self::Clock(_)
            | Self::Signature
            | Self::Manifest
            | Self::Sender => StatusCode::BAD_REQUEST,
            Self::Blocked => StatusCode::FORBIDDEN,
            Self::OverLimit => StatusCode::TOO_MANY_REQUESTS,
            Self::NotSetUp => StatusCode::SERVICE_UNAVAILABLE,
        }
    }
}

/// The manifest of a report that passed every check; its `sender` is the
/// key that signed it.
pub(crate) fn check(headers: &HeaderMap, body: &[u8], now_ms: u64) -> Result<Manifest, Refused> {
    if body.len() > MAX_BODY {
        return Err(Refused::TooLarge);
    }
    let key: EndpointId = header(headers, KEY_HEADER)?;
    let at_ms: u64 = header(headers, TIME_HEADER)?;
    let signature = signature(headers)?;
    let off_ms = now_ms.abs_diff(at_ms);
    if u128::from(off_ms) > MAX_AGE.as_millis() {
        return Err(Refused::Clock(
            (off_ms.saturating_add(30_000) / 60_000).max(1),
        ));
    }
    key.verify(&signed_message(body, at_ms), &signature)
        .map_err(|_| Refused::Signature)?;
    let manifest = read_manifest(body).map_err(|_| Refused::Manifest)?;
    if manifest.sender != key {
        return Err(Refused::Sender);
    }
    Ok(manifest)
}

fn header<T: FromStr>(headers: &HeaderMap, name: &'static str) -> Result<T, Refused> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .and_then(|text| text.parse().ok())
        .ok_or(Refused::Header(name))
}

fn signature(headers: &HeaderMap) -> Result<Signature, Refused> {
    let text: String = header(headers, SIGNATURE_HEADER)?;
    let bytes: [u8; Signature::LENGTH] = hex::decode(text)
        .ok()
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or(Refused::Header(SIGNATURE_HEADER))?;
    Ok(Signature::from_bytes(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;
    use beep_report::{Period, Sender, draft, zip};
    use iroh::SecretKey;

    const NOW_MS: u64 = 1_800_000_000_000;

    fn report(key: &SecretKey) -> Vec<u8> {
        let sender = Sender {
            id: key.public(),
            mesh: None,
            app: "desktop",
            app_version: "0.2.0".into(),
        };
        let draft = draft("it broke", Period::Today, 0, sender, Vec::new()).expect("a draft");
        zip(&draft).expect("a zip")
    }

    fn signed(key: &SecretKey, body: &[u8], at_ms: u64) -> HeaderMap {
        let signature = key.sign(&signed_message(body, at_ms));
        let mut headers = HeaderMap::new();
        for (name, value) in [
            (KEY_HEADER, key.public().to_string()),
            (TIME_HEADER, at_ms.to_string()),
            (SIGNATURE_HEADER, hex::encode(signature.to_bytes())),
        ] {
            let value = HeaderValue::from_str(&value).expect("ASCII header values");
            headers.insert(name, value);
        }
        headers
    }

    #[test]
    fn a_signed_report_passes() {
        let key = SecretKey::generate();
        let body = report(&key);
        let manifest = check(&signed(&key, &body, NOW_MS), &body, NOW_MS).expect("it passes");
        assert_eq!(manifest.sender, key.public());
    }

    #[test]
    fn a_report_from_the_future_is_refused_too() {
        let key = SecretKey::generate();
        let body = report(&key);
        let ahead = NOW_MS + 6 * 60_000;
        let refused = check(&signed(&key, &body, ahead), &body, NOW_MS).map(|m| m.sender);
        assert_eq!(refused, Err(Refused::Clock(6)));
        assert_eq!(
            Refused::Clock(6).to_string(),
            "This computer's clock is off by 6 minutes; set it right and send again."
        );
    }

    #[test]
    fn a_missing_header_is_named() {
        let key = SecretKey::generate();
        let body = report(&key);
        let mut headers = signed(&key, &body, NOW_MS);
        headers.remove(TIME_HEADER);
        let refused = check(&headers, &body, NOW_MS).map(|m| m.sender);
        assert_eq!(refused, Err(Refused::Header(TIME_HEADER)));
    }
}
