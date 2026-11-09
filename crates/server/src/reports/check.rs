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
    #[error("Beep is taking in many reports right now; try again in a minute.")]
    Busy,
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
            Self::NotSetUp | Self::Busy => StatusCode::SERVICE_UNAVAILABLE,
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
#[path = "../tests/reports_check.rs"]
mod tests;
