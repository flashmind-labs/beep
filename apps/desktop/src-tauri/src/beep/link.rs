//! Linking to a phone: a signed offer for the server, shown as a code and
//! a QR code.

use crate::{
    beep::{Beep, identity::secret_key},
    error::{DesktopError, Result},
};
use beep_proto::link::LinkOffer;
use qrcode::{QrCode, render::svg::Color};
use serde::Serialize;
use std::time::SystemTime;

/// A code for the phone to scan.
#[derive(Debug, Clone, Serialize)]
pub struct LinkView {
    pub uri: String,
    pub code: String,
    pub qr_svg: String,
    pub expires_in: u64,
}

impl Beep {
    /// A fresh offer, signed for this server.
    pub fn link_offer(&self, name: &str) -> Result<LinkView> {
        let server = self.server().ok_or(DesktopError::NoServer)?;
        let offer = LinkOffer::sign(&secret_key(&self.identity), server, name, SystemTime::now())
            .map_err(|e| DesktopError::BadRequest(e.to_string()))?;
        let uri = offer.to_uri();
        let qr_svg = qr_svg(&uri)?;
        let expires_in = offer
            .expires_at()
            .duration_since(SystemTime::now())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        Ok(LinkView {
            code: offer.code(),
            uri,
            qr_svg,
            expires_in,
        })
    }
}

/// `data` as a QR code the bar can draw.
pub fn qr_svg(data: &str) -> Result<String> {
    let qr = QrCode::new(data.as_bytes()).map_err(|e| DesktopError::BadRequest(e.to_string()))?;
    Ok(qr
        .render::<Color>()
        .quiet_zone(false)
        .dark_color(Color("#0A0E0C"))
        .light_color(Color("#F4F8F5"))
        .build())
}
