//! The apps' side of `http`: one `Client` per server, signed in as this
//! device's key.

use crate::{
    ApiError, AppEvent, Call, Duplex, Platform, Stream,
    http::{
        self, CONTENT_TYPE, ERR, EVENTS_PATH, HEARTBEAT, OK, PLATFORM_HEADER, REFUSED_HEADER,
        Refusal, SERVER_PATH, SESSION_PATH, ServerInfo, SessionStart, SessionToken, VERSION_HEADER,
    },
};
use anyhow::{Context, Result, anyhow, bail};
use bytes::Bytes;
use futures::{StreamExt, TryStreamExt, stream::BoxStream};
use iroh::{EndpointId, SecretKey};
use reqwest::{
    Body, Method, RequestBuilder, Response, StatusCode,
    header::{AUTHORIZATION, CONTENT_TYPE as CONTENT_TYPE_HEADER},
};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    io,
    marker::PhantomData,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::{
    sync::{OnceCell, mpsc},
    time::timeout,
};
use tokio_util::{codec::FramedRead, io::StreamReader};

pub type AsyncMutex<T> = tokio::sync::Mutex<T>;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// A token is renewed this long before it runs out.
const RENEW_BEFORE: Duration = Duration::from_secs(60);
/// A quiet events stream is given up after this: two heartbeats missed.
pub const EVENTS_SILENCE: Duration = Duration::from_secs(HEARTBEAT.as_secs() * 5 / 2);
/// `Duplex` frames waiting to go up.
const UPLINK_QUEUE: usize = 16;

/// A session token and when to stop using it.
struct Token {
    bearer: String,
    renew_at: Instant,
}

/// One server, reached over HTTP as `identity`. Requests run concurrently;
/// errors the server sends back are `ApiError`, and a refused key is
/// `Refusal`, both recoverable by downcast.
pub struct Client {
    http: reqwest::Client,
    base: String,
    identity: SecretKey,
    platform: Option<Platform>,
    version: Option<String>,
    server: OnceCell<ServerInfo>,
    /// Held while a token is made, so concurrent requests make one.
    token: AsyncMutex<Option<Token>>,
}

impl Client {
    /// A client of the server at `base`, such as `https://example.com`. Plain
    /// `http://` speaks HTTP/2 without TLS, for a server on this machine.
    pub fn new(base: &str, identity: SecretKey) -> Result<Self> {
        let base = base.trim().trim_end_matches('/').to_owned();
        let mut builder = reqwest::Client::builder().connect_timeout(CONNECT_TIMEOUT);
        if base.starts_with("http://") {
            builder = builder.http2_prior_knowledge();
        } else if !base.starts_with("https://") {
            bail!("the server address must start with https://");
        }
        Ok(Self {
            http: builder.build()?,
            base,
            identity,
            platform: None,
            version: None,
            server: OnceCell::new(),
            token: AsyncMutex::new(None),
        })
    }

    /// Names the app on each request: which one it is and its own version,
    /// such as "0.1.0 (142)".
    pub fn with_app(mut self, platform: Platform, version: impl Into<String>) -> Self {
        self.platform = Some(platform);
        self.version = Some(version.into());
        self
    }

    /// This device's key, which the server knows it by.
    pub fn id(&self) -> EndpointId {
        self.identity.public()
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    /// The server's key and whether it has hosted sign-in. Asks once; makes
    /// no account.
    pub async fn server_info(&self) -> Result<&ServerInfo> {
        self.server
            .get_or_try_init(|| async {
                let response = self
                    .http
                    .get(self.url(SERVER_PATH))
                    .send()
                    .await
                    .context("the server did not answer")?;
                let body = ok_body(response).await?;
                http::decode(&body).context("could not read the server's reply")
            })
            .await
    }

    /// The server's key, which link offers are signed for.
    pub async fn server_id(&self) -> Result<EndpointId> {
        let info = self.server_info().await?;
        info.id.parse().context("the server sent a bad key")
    }

    /// Whether the server has hosted sign-in. An unreachable one reads as no.
    pub async fn has_login(&self) -> bool {
        self.server_info().await.is_ok_and(|info| info.login)
    }

    /// Sends a request and returns its reply.
    pub async fn call<R: Call>(&self, request: &R) -> Result<R::Reply> {
        let body = http::encode(request)?;
        let response = self.post(&http::path(R::OP), body).await?;
        let reply = ok_body(response).await?;
        http::decode(&reply).context("could not read the server's reply")
    }

    /// Sends a request and returns its replies as they arrive.
    pub async fn stream<R: Stream>(&self, request: &R) -> Result<Replies<R::Reply>> {
        let body = http::encode(request)?;
        let response = self.post(&http::path(R::OP), body).await?;
        Ok(Replies::new(checked(response).await?))
    }

    /// Sends a request that keeps sending: `Uplink` carries what follows it,
    /// and the replies arrive as they come.
    pub async fn duplex<R: Duplex>(
        &self,
        request: &R,
    ) -> Result<(Uplink<R::Up>, Replies<R::Reply>)> {
        let (tx, mut rx) = mpsc::channel(UPLINK_QUEUE);
        let mut uplink = Uplink {
            tx: Some(tx),
            _up: PhantomData,
        };
        uplink.send_bytes(&http::encode(request)?).await?;
        // The body cannot be sent twice, so the token is made first.
        let token = self.bearer().await?;
        let body = Body::wrap_stream(futures::stream::poll_fn(move |cx| rx.poll_recv(cx)));
        let response = self
            .request(Method::POST, &http::path(R::OP), &token)
            .body(body)
            .send()
            .await
            .context("the server did not answer")?;
        Ok((uplink, Replies::new(checked(response).await?)))
    }

    /// Opens the events stream, after the notice `after` when given.
    pub async fn events(&self, after: Option<i64>) -> Result<Events> {
        let path = match after {
            Some(after) => format!("{EVENTS_PATH}?after={after}"),
            None => EVENTS_PATH.to_owned(),
        };
        let response = self.send(Method::GET, &path, None).await?;
        Ok(Events {
            frames: frames(checked(response).await?),
            silence: EVENTS_SILENCE,
        })
    }

    async fn post(&self, path: &str, body: Vec<u8>) -> Result<Response> {
        self.send(Method::POST, path, Some(Bytes::from(body))).await
    }

    /// Sends with the session token, making a new one once when the server
    /// says it is stale.
    async fn send(&self, method: Method, path: &str, body: Option<Bytes>) -> Result<Response> {
        for attempt in 0..2 {
            let token = self.bearer().await?;
            let mut request = self.request(method.clone(), path, &token);
            if let Some(body) = &body {
                request = request.body(body.clone());
            }
            let response = request.send().await.context("the server did not answer")?;
            if response.status() == StatusCode::UNAUTHORIZED && attempt == 0 {
                self.forget_token(&token).await;
                continue;
            }
            return Ok(response);
        }
        bail!("the server refused every session token")
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    fn request(&self, method: Method, path: &str, token: &str) -> RequestBuilder {
        let mut request = self
            .http
            .request(method, self.url(path))
            .header(AUTHORIZATION, format!("Bearer {token}"))
            .header(CONTENT_TYPE_HEADER, CONTENT_TYPE);
        if let Some(platform) = self.platform {
            request = request.header(PLATFORM_HEADER, platform.as_header());
        }
        if let Some(version) = &self.version {
            request = request.header(VERSION_HEADER, version);
        }
        request
    }

    /// A session token good for now, made if there is none.
    async fn bearer(&self) -> Result<String> {
        let mut token = self.token.lock().await;
        if let Some(token) = token.as_ref().filter(|t| Instant::now() < t.renew_at) {
            return Ok(token.bearer.clone());
        }
        let made = self.start_session().await?;
        let bearer = made.bearer.clone();
        *token = Some(made);
        Ok(bearer)
    }

    async fn forget_token(&self, stale: &str) {
        let mut token = self.token.lock().await;
        if token.as_ref().is_some_and(|t| t.bearer == stale) {
            *token = None;
        }
    }

    async fn start_session(&self) -> Result<Token> {
        let server = self.server_id().await?;
        let at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let key = self.identity.public();
        let signature = self
            .identity
            .sign(&http::session_message(&server, &key, at));
        let start = SessionStart {
            key: key.to_string(),
            at,
            signature: signature.to_bytes().to_vec(),
        };
        let response = self
            .http
            .post(self.url(SESSION_PATH))
            .header(CONTENT_TYPE_HEADER, CONTENT_TYPE)
            .body(http::encode(&start)?)
            .send()
            .await
            .context("the server did not answer")?;
        let body = ok_body(response).await?;
        let session: SessionToken =
            http::decode(&body).context("could not read the session token")?;
        let lasts = Duration::from_secs(session.expires_in).saturating_sub(RENEW_BEFORE);
        Ok(Token {
            bearer: session.token.expose().to_owned(),
            renew_at: Instant::now() + lasts,
        })
    }
}

/// The response when it succeeded, else the error it carries.
async fn checked(response: Response) -> Result<Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    if let Some(refusal) = response
        .headers()
        .get(REFUSED_HEADER)
        .and_then(|value| value.to_str().ok())
        .and_then(Refusal::parse)
    {
        return Err(refusal.into());
    }
    let body = response.bytes().await.unwrap_or_default();
    match http::decode::<ApiError>(&body) {
        Ok(error) => Err(error.into()),
        Err(_) => Err(anyhow!("the server answered {status}")),
    }
}

async fn ok_body(response: Response) -> Result<Bytes> {
    let response = checked(response).await?;
    Ok(response.bytes().await?)
}

type Frames = FramedRead<
    StreamReader<BoxStream<'static, io::Result<Bytes>>, Bytes>,
    tokio_util::codec::LengthDelimitedCodec,
>;

fn frames(response: Response) -> Frames {
    let body = response.bytes_stream().map_err(io::Error::other).boxed();
    FramedRead::new(StreamReader::new(body), http::codec())
}

/// The replies of a `Stream` or `Duplex` request.
pub struct Replies<T> {
    frames: Frames,
    _reply: PhantomData<fn() -> T>,
}

impl<T: DeserializeOwned> Replies<T> {
    fn new(response: Response) -> Self {
        Self {
            frames: frames(response),
            _reply: PhantomData,
        }
    }

    /// Next reply, `None` at end of stream. An error frame is an `ApiError`.
    pub async fn next(&mut self) -> Result<Option<T>> {
        let Some(frame) = self.frames.next().await else {
            return Ok(None);
        };
        let frame = frame.context("the stream broke")?;
        let (&tag, body) = frame.split_first().context("empty frame")?;
        match tag {
            OK => Ok(Some(
                http::decode(body).context("could not read the server's reply")?,
            )),
            ERR => Err(http::decode::<ApiError>(body)
                .context("could not read the server's error")?
                .into()),
            tag => bail!("unknown frame tag {tag}"),
        }
    }
}

/// The sending side of a `Duplex` request. Finish it to say nothing more
/// comes; dropping it does the same.
pub struct Uplink<T> {
    tx: Option<mpsc::Sender<io::Result<Bytes>>>,
    _up: PhantomData<fn(T)>,
}

impl<T: Serialize> Uplink<T> {
    pub async fn send(&mut self, up: &T) -> Result<()> {
        self.send_bytes(&http::encode(up)?).await
    }

    pub fn finish(&mut self) -> Result<()> {
        self.tx = None;
        Ok(())
    }

    async fn send_bytes(&mut self, body: &[u8]) -> Result<()> {
        let tx = self.tx.as_ref().context("the uplink is finished")?;
        tx.send(Ok(http::length_prefixed(body)))
            .await
            .map_err(|_| anyhow!("the request ended"))
    }
}

/// The events stream. It fails once the server goes quiet for longer than
/// its heartbeat allows.
pub struct Events {
    frames: Frames,
    silence: Duration,
}

impl Events {
    /// Next event, `None` when the server ended the stream.
    pub async fn next(&mut self) -> Result<Option<AppEvent>> {
        let frame = match timeout(self.silence, self.frames.next()).await {
            Ok(frame) => frame,
            Err(_) => bail!(
                "the server sent nothing for {} seconds",
                self.silence.as_secs()
            ),
        };
        let Some(frame) = frame else {
            return Ok(None);
        };
        let frame = frame.context("the events stream broke")?;
        Ok(Some(
            http::decode(&frame).context("could not read an event")?,
        ))
    }
}

/// True when `error` is the server refusing this key.
pub fn refusal(error: &anyhow::Error) -> Option<Refusal> {
    error.downcast_ref::<Refusal>().copied()
}
