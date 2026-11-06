//! Serves the apps over HTTP (see `beep_proto::http`): plain HTTP/1.1 or
//! HTTP/2 without TLS, for Cloudflare or another proxy in front to hold TLS.

use crate::{
    account::guest,
    api::{
        self,
        auth::Tokens,
        router::{Answer, Cx, Kind, Router, UpFrames},
    },
    app::{Admission, App},
    protocol::{
        ApiError, DeviceKind, Op, Platform,
        http::{
            self, CONTENT_TYPE, EVENTS_PATH, MAX_FRAME, PLATFORM_HEADER, REFUSED_HEADER, Refusal,
            SERVER_PATH, SESSION_PATH, ServerInfo, SessionStart,
        },
    },
    push::{Device, Events, Hub, Pulse, notices},
};
use anyhow::Result;
use axum::{
    body::{Body, to_bytes},
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use bytes::Bytes;
use futures::{StreamExt, TryStreamExt};
use iroh::{EndpointId, SecretKey};
use serde::Deserialize;
use std::{
    collections::HashMap,
    convert::Infallible,
    future::{Future, pending},
    io,
    net::Ipv4Addr,
    sync::Arc,
    time::SystemTime,
};
use tokio::{
    net::TcpListener,
    sync::{OwnedSemaphorePermit, Semaphore},
};
use tokio_util::{codec::FramedRead, io::StreamReader, task::AbortOnDropHandle};

/// Requests served at once, open streams included; past it the server
/// answers 503.
const MAX_REQUESTS: usize = 4096;

/// What every request reaches.
struct Service {
    app: Arc<App>,
    router: Router,
    ops: HashMap<String, Op>,
    tokens: Tokens,
    server: EndpointId,
    events: Events,
    permits: Arc<Semaphore>,
}

/// The apps' HTTP service for `app`, signing tokens with the server's `key`.
/// It listens for events from every instance from now on; `pulse` says when
/// open streams send heartbeats.
pub async fn service(app: Arc<App>, key: &SecretKey, pulse: Pulse) -> Result<axum::Router> {
    let hub = Hub::start(app.store.clone()).await?;
    let service = Service {
        events: Events::new(app.store.clone(), hub, pulse),
        app,
        router: api::router(),
        ops: Op::ALL.iter().map(|op| (format!("{op:?}"), *op)).collect(),
        tokens: Tokens::new(key),
        server: key.public(),
        permits: Arc::new(Semaphore::new(MAX_REQUESTS)),
    };
    Ok(axum::Router::new()
        .route(SERVER_PATH, get(server_info))
        .route(SESSION_PATH, post(start_session))
        .route(EVENTS_PATH, get(events))
        .route("/v1/{name}", post(request))
        .with_state(Arc::new(service)))
}

/// Serves `service` on `listener` until `stop` resolves; open streams end then.
pub async fn serve(
    listener: TcpListener,
    service: axum::Router,
    stop: impl Future<Output = ()> + Send + 'static,
) -> Result<()> {
    axum::serve(listener, service)
        .with_graceful_shutdown(stop)
        .await?;
    Ok(())
}

/// The apps' side of a server on a free port of this machine, for tests and
/// tools: the HTTP service and the notice log, with no FCM or APNs. Dropping
/// it stops both.
pub struct Local {
    url: String,
    _serving: AbortOnDropHandle<Result<()>>,
    _notices: AbortOnDropHandle<()>,
}

impl Local {
    pub async fn start(app: Arc<App>, key: &SecretKey, pulse: Pulse) -> Result<Self> {
        let notices = notices::start(
            app.store.clone(),
            app.notifications.subscribe(),
            None,
            notices::ACK_WAIT,
        );
        let service = service(app, key, pulse).await?;
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let url = format!("http://{}", listener.local_addr()?);
        let serving = tokio::spawn(serve(listener, service, pending()));
        Ok(Self {
            url,
            _serving: AbortOnDropHandle::new(serving),
            _notices: AbortOnDropHandle::new(notices),
        })
    }

    /// Where a client reaches it: `http://127.0.0.1:<port>`.
    pub fn url(&self) -> &str {
        &self.url
    }
}

async fn server_info(State(service): State<Arc<Service>>) -> Response {
    let info = ServerInfo {
        id: service.server.to_string(),
        login: service.app.login().is_some(),
    };
    msgpack(StatusCode::OK, &info)
}

async fn start_session(State(service): State<Arc<Service>>, body: Bytes) -> Response {
    let start = match http::decode::<SessionStart>(&body) {
        Ok(start) => start,
        Err(e) => return error(&ApiError::bad_request(e.to_string())),
    };
    match service.tokens.start(&start, SystemTime::now()) {
        Ok(token) => msgpack(StatusCode::OK, &token),
        Err(ApiError::Unauthorized {}) => stale_token(),
        Err(e) => error(&e),
    }
}

#[derive(Deserialize)]
struct After {
    after: Option<i64>,
}

async fn events(
    State(service): State<Arc<Service>>,
    headers: HeaderMap,
    Query(After { after }): Query<After>,
) -> Response {
    let cx = match service.admit(&headers).await {
        Ok(Caller::SignedIn(cx)) => cx,
        Ok(Caller::Guest { .. }) => return error(&ApiError::Unauthorized {}),
        Err(refused) => return refused.response(),
    };
    let Some(permit) = service.permit() else {
        return busy();
    };
    let device = Device {
        user: cx.user,
        key: cx.key,
        kind: cx.device,
    };
    streaming(service.events.open(device, after), permit)
}

async fn request(
    State(service): State<Arc<Service>>,
    Path(name): Path<String>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let Some(&op) = service.ops.get(&name) else {
        return error(&ApiError::bad_request(format!("{name} is not a request")));
    };
    let caller = match service.admit(&headers).await {
        Ok(caller) => caller,
        Err(refused) => return refused.response(),
    };
    let Some(permit) = service.permit() else {
        return busy();
    };
    let (first, up) = match read_body(service.router.kind(op), body).await {
        Ok(read) => read,
        Err(e) => return error(&e),
    };
    let answer = match caller {
        Caller::SignedIn(cx) => service.router.dispatch(op, cx, first, up).await,
        Caller::Guest { key, platform } => {
            Answer::Once(guest::call(&service.app, key, platform, op, &first).await)
        }
    };
    match answer {
        Answer::Once(Ok(reply)) => {
            drop(permit);
            raw(StatusCode::OK, reply)
        }
        Answer::Once(Err(e)) => error(&e),
        Answer::Frames(frames) => streaming(frames, permit),
    }
}

/// The request, and for a `Duplex` the frames that follow it.
async fn read_body(kind: Option<Kind>, body: Body) -> Result<(Bytes, Option<UpFrames>), ApiError> {
    if kind != Some(Kind::Duplex) {
        let bytes = to_bytes(body, MAX_FRAME)
            .await
            .map_err(|_| ApiError::bad_request("the request is too large or was cut off"))?;
        return Ok((bytes, None));
    }
    let stream = body.into_data_stream().map_err(io::Error::other).boxed();
    let mut frames = FramedRead::new(StreamReader::new(stream), http::codec());
    let first = frames
        .next()
        .await
        .ok_or_else(|| ApiError::bad_request("the request is empty"))?
        .map_err(|_| ApiError::bad_request("the request is too large or was cut off"))?;
    Ok((first.freeze(), Some(frames)))
}

/// Who sent a request.
enum Caller {
    SignedIn(Cx),
    /// A key with no account, which may only join one or sign in.
    Guest {
        key: EndpointId,
        platform: Option<Platform>,
    },
}

/// Why a request is not served.
enum Refused {
    /// No token, or one past its time: get a new one.
    Stale,
    Key(Refusal),
    Busy,
}

impl Refused {
    fn response(self) -> Response {
        match self {
            Self::Stale => stale_token(),
            Self::Key(refusal) => refused(refusal),
            Self::Busy => busy(),
        }
    }
}

impl Service {
    /// The caller, or why it is refused.
    async fn admit(&self, headers: &HeaderMap) -> Result<Caller, Refused> {
        let key = headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .and_then(|token| self.tokens.check(token, SystemTime::now()))
            .ok_or(Refused::Stale)?;
        let platform = headers
            .get(PLATFORM_HEADER)
            .and_then(|value| value.to_str().ok())
            .and_then(Platform::from_header);
        let signed_in = match self.app.admit(&key).await {
            Ok(Admission::SignedIn(signed_in)) => signed_in,
            Ok(Admission::Guest) => return Ok(Caller::Guest { key, platform }),
            Ok(Admission::Revoked) => return Err(Refused::Key(Refusal::Revoked)),
            Ok(Admission::NotInvited) => return Err(Refused::Key(Refusal::NotInvited)),
            Err(e) => {
                log::warn!("sign in failed: {e:#}");
                return Err(Refused::Busy);
            }
        };
        // A key makes its account as a phone; one the Mac app made it with
        // is a computer.
        let mut device = signed_in.device;
        if signed_in.primary
            && matches!(device, DeviceKind::Phone {})
            && matches!(platform, Some(Platform::Macos {}))
        {
            device = DeviceKind::Desktop {};
            if let Err(e) = self.app.store.set_key_kind(&key, device).await {
                log::warn!("unable to mark a key as a computer: {e:#}");
            }
        }
        Ok(Caller::SignedIn(Cx {
            app: self.app.clone(),
            user: signed_in.user,
            device,
            primary: signed_in.primary,
            key,
            server: self.server,
            platform,
        }))
    }

    fn permit(&self) -> Option<OwnedSemaphorePermit> {
        self.permits.clone().try_acquire_owned().ok()
    }
}

fn raw(status: StatusCode, body: Vec<u8>) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, HeaderValue::from_static(CONTENT_TYPE))],
        body,
    )
        .into_response()
}

fn msgpack<T: serde::Serialize>(status: StatusCode, value: &T) -> Response {
    match http::encode(value) {
        Ok(body) => raw(status, body),
        Err(e) => {
            log::warn!("a reply did not encode: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

fn error(error: &ApiError) -> Response {
    let status = StatusCode::from_u16(http::status(error)).unwrap_or(StatusCode::BAD_REQUEST);
    msgpack(status, error)
}

/// The token is missing, stale or not this server's: get a new one.
fn stale_token() -> Response {
    msgpack(StatusCode::UNAUTHORIZED, &ApiError::Unauthorized {})
}

fn refused(refusal: Refusal) -> Response {
    let mut response = error(&ApiError::Unauthorized {});
    response
        .headers_mut()
        .insert(REFUSED_HEADER, HeaderValue::from_static(refusal.as_str()));
    response
}

fn busy() -> Response {
    msgpack(StatusCode::SERVICE_UNAVAILABLE, &ApiError::Internal {})
}

/// A body of `frames`, holding `permit` until it ends.
fn streaming(
    frames: futures::stream::BoxStream<'static, Bytes>,
    permit: OwnedSemaphorePermit,
) -> Response {
    let body = frames.map(move |frame| {
        let _held = &permit;
        Ok::<_, Infallible>(frame)
    });
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, HeaderValue::from_static(CONTENT_TYPE)),
            // Proxies pass each frame on at once.
            (header::CACHE_CONTROL, HeaderValue::from_static("no-cache")),
            (
                header::HeaderName::from_static("x-accel-buffering"),
                HeaderValue::from_static("no"),
            ),
        ],
        Body::from_stream(body),
    )
        .into_response()
}
