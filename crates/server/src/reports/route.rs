//! `POST /v1/reports`: no sign-in, the device key's signature instead.

use std::collections::HashSet;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use axum::body::Bytes;
use axum::extract::rejection::BytesRejection;
use axum::extract::{ConnectInfo, DefaultBodyLimit, State};
use axum::http::header::CONTENT_TYPE;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Extension, Router};
use beep_proto::ApiError;
use beep_proto::report::{MAX_BODY, PATH, Receipt, report_id};
use chrono::{DateTime, NaiveDate};
use iroh::EndpointId;
use reqwest::Client;
use serde::Serialize;

use super::check::{Refused, check};
use super::limit::KeyLimit;
use super::store::ReportStore;
use crate::config::ReportsConfig;

/// The address Cloudflare saw the request come from.
const CLIENT_IP_HEADER: &str = "cf-connecting-ip";

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
    /// Starts the task that counts each key's reports, so it needs a runtime.
    pub fn new(config: &ReportsConfig, http: Client) -> Result<Self> {
        Ok(Self {
            inner: Arc::new(Inner {
                store: ReportStore::from_config(config, http)?,
                blocked_keys: config.blocked_keys.iter().copied().collect(),
                blocked_ips: config.blocked_ips.iter().copied().collect(),
                limit: KeyLimit::spawn(config.per_key_per_day),
            }),
        })
    }
}

pub fn routes(reports: Reports) -> Router {
    Router::new()
        .route(PATH, post(receive))
        // One byte over, so `check` is the one that refuses a body too large.
        .layer(DefaultBodyLimit::max(MAX_BODY + 1))
        .with_state(reports)
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

/// Checks the report and stores it; the outer error is the store failing.
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
        Err(e) => return Err(e).context("reading the report"),
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
    if !reports.limit.take(manifest.sender, day(now_ms)).await? {
        return Ok(Err(Refused::OverLimit));
    }
    let id = report_id(&body);
    let folder = format!("reports/{}", manifest.mesh.unwrap_or(manifest.sender));
    let json = serde_json::to_vec_pretty(&manifest)?;
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
    Ok(Ok(Receipt {
        reference: format!("R-{}", id.to_uppercase()),
    }))
}

/// Cloudflare's `CF-Connecting-IP` when it reads, else the peer's address.
fn client_ip(headers: &HeaderMap, peer: Option<IpAddr>) -> Option<IpAddr> {
    headers
        .get(CLIENT_IP_HEADER)
        .and_then(|value| value.to_str().ok())
        .and_then(|text| text.trim().parse().ok())
        .or(peer)
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
