//! `POST /v1/reports`: no sign-in, the device key's signature instead.

use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use axum::body::Bytes;
use axum::extract::rejection::BytesRejection;
use axum::extract::{ConnectInfo, DefaultBodyLimit, Request, State};
use axum::http::header::CONTENT_TYPE;
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::{Next, from_fn_with_state};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Extension, Router};
use beep_proto::ApiError;
use beep_proto::report::{MAX_BODY, PATH, Receipt, report_id};
use beep_report::Manifest;
use chrono::{DateTime, NaiveDate};
use iroh::EndpointId;
use reqwest::Client;
use serde::Serialize;
use tokio::sync::Semaphore;

use super::check::{Refused, check};
use super::limit::KeyLimit;
use super::store::ReportStore;
use crate::config::ReportsConfig;

/// The address Cloudflare saw the request come from.
const CLIENT_IP_HEADER: &str = "cf-connecting-ip";

/// Reports read and stored at once; past it the route answers 503 before
/// reading the body.
const MAX_UPLOADS: usize = 8;

/// Cloudflare's published ranges (https://www.cloudflare.com/ips/): only a
/// peer in one of them, or on this machine (a Tunnel), names the client.
const CLOUDFLARE_V4: [(Ipv4Addr, u8); 15] = [
    (Ipv4Addr::new(173, 245, 48, 0), 20),
    (Ipv4Addr::new(103, 21, 244, 0), 22),
    (Ipv4Addr::new(103, 22, 200, 0), 22),
    (Ipv4Addr::new(103, 31, 4, 0), 22),
    (Ipv4Addr::new(141, 101, 64, 0), 18),
    (Ipv4Addr::new(108, 162, 192, 0), 18),
    (Ipv4Addr::new(190, 93, 240, 0), 20),
    (Ipv4Addr::new(188, 114, 96, 0), 20),
    (Ipv4Addr::new(197, 234, 240, 0), 22),
    (Ipv4Addr::new(198, 41, 128, 0), 17),
    (Ipv4Addr::new(162, 158, 0, 0), 15),
    (Ipv4Addr::new(104, 16, 0, 0), 13),
    (Ipv4Addr::new(104, 24, 0, 0), 14),
    (Ipv4Addr::new(172, 64, 0, 0), 13),
    (Ipv4Addr::new(131, 0, 72, 0), 22),
];
const CLOUDFLARE_V6: [(Ipv6Addr, u8); 7] = [
    (Ipv6Addr::new(0x2400, 0xcb00, 0, 0, 0, 0, 0, 0), 32),
    (Ipv6Addr::new(0x2606, 0x4700, 0, 0, 0, 0, 0, 0), 32),
    (Ipv6Addr::new(0x2803, 0xf800, 0, 0, 0, 0, 0, 0), 32),
    (Ipv6Addr::new(0x2405, 0xb500, 0, 0, 0, 0, 0, 0), 32),
    (Ipv6Addr::new(0x2405, 0x8100, 0, 0, 0, 0, 0, 0), 32),
    (Ipv6Addr::new(0x2a06, 0x98c0, 0, 0, 0, 0, 0, 0), 29),
    (Ipv6Addr::new(0x2c0f, 0xf248, 0, 0, 0, 0, 0, 0), 32),
];

/// What the route needs: the store, the block lists and the day's counts.
#[derive(Clone)]
pub struct Reports {
    inner: Arc<Inner>,
}

struct Inner {
    store: Option<ReportStore>,
    blocked_keys: HashSet<EndpointId>,
    blocked_ips: HashSet<IpAddr>,
    limit: KeyLimit,
}

impl Reports {
    /// Starts the task that counts each key's reports; fails outside a
    /// tokio runtime.
    pub fn new(config: &ReportsConfig, http: Client) -> Result<Self> {
        Ok(Self {
            inner: Arc::new(Inner {
                store: ReportStore::from_config(config, http)?,
                blocked_keys: config.blocked_keys.iter().copied().collect(),
                blocked_ips: config.blocked_ips.iter().copied().collect(),
                limit: KeyLimit::spawn(config.per_key_per_day)?,
            }),
        })
    }
}

pub fn routes(reports: Reports) -> Router {
    Router::new()
        .route(PATH, post(receive))
        // One byte over, so `check` is the one that refuses a body too large.
        .layer(DefaultBodyLimit::max(MAX_BODY + 1))
        // Outside the body limit: a permit comes before any body is read.
        .layer(from_fn_with_state(
            Arc::new(Semaphore::new(MAX_UPLOADS)),
            one_of_few,
        ))
        .with_state(reports)
}

/// Lets `MAX_UPLOADS` reports through at once and turns the rest away.
async fn one_of_few(
    State(uploads): State<Arc<Semaphore>>,
    request: Request,
    next: Next,
) -> Response {
    match uploads.try_acquire() {
        Ok(_permit) => next.run(request).await,
        Err(_) => reply(
            StatusCode::SERVICE_UNAVAILABLE,
            &ApiError::bad_request(Refused::Busy.to_string()),
        ),
    }
}

async fn receive(
    State(reports): State<Reports>,
    peer: Option<Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let peer = peer.map(|Extension(ConnectInfo(addr))| addr.ip());
    match take(&reports.inner, peer, &headers, body).await {
        Ok(Ok(receipt)) => reply(StatusCode::OK, &receipt),
        Ok(Err(refused)) => reply(
            refused.status(),
            &ApiError::bad_request(refused.to_string()),
        ),
        Err(e) => {
            log::warn!("a problem report was not stored: {e:#}");
            reply(StatusCode::INTERNAL_SERVER_ERROR, &ApiError::Internal {})
        }
    }
}

/// Checks the report and stores it. The outer error is the store failing,
/// which gives the report back to the day's count.
async fn take(
    reports: &Inner,
    peer: Option<IpAddr>,
    headers: &HeaderMap,
    body: Result<Bytes, BytesRejection>,
) -> Result<Result<Receipt, Refused>> {
    let Some(store) = &reports.store else {
        return Ok(Err(Refused::NotSetUp));
    };
    let body = match body {
        Ok(body) => body,
        Err(rejection) if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE => {
            return Ok(Err(Refused::TooLarge));
        }
        Err(_) => return Ok(Err(Refused::Body)),
    };
    if client_ip(headers, peer).is_some_and(|ip| reports.blocked_ips.contains(&ip)) {
        return Ok(Err(Refused::Blocked));
    }
    let now_ms = now_ms();
    let manifest = match check(headers, &body, now_ms) {
        Ok(manifest) => manifest,
        Err(refused) => return Ok(Err(refused)),
    };
    if reports.blocked_keys.contains(&manifest.sender) {
        return Ok(Err(Refused::Blocked));
    }
    let day = day(now_ms);
    if !reports.limit.take(manifest.sender, day).await? {
        return Ok(Err(Refused::OverLimit));
    }
    match keep(store, &manifest, body).await {
        Ok(id) => Ok(Ok(Receipt {
            reference: format!("R-{}", id.to_uppercase()),
        })),
        Err(e) => {
            reports.limit.give_back(manifest.sender, day).await?;
            Err(e)
        }
    }
}

/// Puts the zip and its manifest in the store; returns the report's id.
async fn keep(store: &ReportStore, manifest: &Manifest, body: Bytes) -> Result<String> {
    let id = report_id(&body);
    let folder = format!("reports/{}", manifest.mesh.unwrap_or(manifest.sender));
    let json = serde_json::to_vec_pretty(manifest)?;
    // The zip first: a listing reads the .json, so a listed report has its zip.
    store
        .put(
            &format!("{folder}/{id}.zip"),
            body.into(),
            "application/zip",
        )
        .await?;
    store
        .put(&format!("{folder}/{id}.json"), json, "application/json")
        .await?;
    Ok(id)
}

/// Cloudflare's `CF-Connecting-IP` when the peer is Cloudflare or this
/// machine and the header reads, else the peer's address.
fn client_ip(headers: &HeaderMap, peer: Option<IpAddr>) -> Option<IpAddr> {
    if !peer.is_some_and(from_cloudflare) {
        return peer;
    }
    headers
        .get(CLIENT_IP_HEADER)
        .and_then(|value| value.to_str().ok())
        .and_then(|text| text.trim().parse().ok())
        .or(peer)
}

/// A Tunnel's `cloudflared` on this machine, or Cloudflare's own edge.
fn from_cloudflare(peer: IpAddr) -> bool {
    match peer.to_canonical() {
        IpAddr::V4(ip) => {
            ip.is_loopback()
                || CLOUDFLARE_V4.iter().any(|&(net, bits)| {
                    let mask = u32::MAX << (32 - u32::from(bits));
                    u32::from(ip) & mask == u32::from(net)
                })
        }
        IpAddr::V6(ip) => {
            ip.is_loopback()
                || CLOUDFLARE_V6.iter().any(|&(net, bits)| {
                    let mask = u128::MAX << (128 - u32::from(bits));
                    u128::from(ip) & mask == u128::from(net)
                })
        }
    }
}

fn now_ms() -> u64 {
    // A clock before 1970 reads as 0, and every report is then off.
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

/// The UTC date of `ms`.
fn day(ms: u64) -> NaiveDate {
    i64::try_from(ms)
        .ok()
        .and_then(DateTime::from_timestamp_millis)
        .unwrap_or_default()
        .date_naive()
}

fn reply<T: Serialize>(status: StatusCode, value: &T) -> Response {
    match rmp_serde::to_vec_named(value) {
        Ok(bytes) => (status, [(CONTENT_TYPE, "application/msgpack")], bytes).into_response(),
        Err(e) => {
            log::warn!("a reports reply did not encode: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn from(ip: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            CLIENT_IP_HEADER,
            HeaderValue::from_str(ip).expect("an ASCII value"),
        );
        headers
    }

    #[test]
    fn the_header_names_the_client_only_from_cloudflare_or_a_tunnel() {
        let client: IpAddr = "198.51.100.1".parse().expect("an address");
        let headers = from("198.51.100.1");
        for peer in [
            "127.0.0.1",
            "::1",
            "104.16.1.2",
            "2606:4700::1",
            "::ffff:172.64.0.9",
        ] {
            let peer: IpAddr = peer.parse().expect("an address");
            assert_eq!(client_ip(&headers, Some(peer)), Some(client), "{peer}");
        }
        for peer in ["203.0.113.5", "104.32.0.1", "2001:db8::1"] {
            let peer: IpAddr = peer.parse().expect("an address");
            assert_eq!(client_ip(&headers, Some(peer)), Some(peer), "{peer}");
        }
        assert_eq!(client_ip(&headers, None), None);
    }
}
