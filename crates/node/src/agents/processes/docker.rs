//! A session's own Docker socket. The node listens on it for the session's
//! agent, which finds it in `DOCKER_HOST`, and passes every request to the
//! daemon. Each container the agent creates gets the session's label, so
//! the session lists the containers it started, with their published ports,
//! though their servers run under the daemon and never in its process tree.
//! Only a daemon on a Unix socket is reached this way; elsewhere the agent's
//! Docker is left as it was and the session lists no containers.

#[cfg(unix)]
pub(crate) use unix::Docker;

/// Never made where Docker is not reached over a Unix socket.
#[cfg(not(unix))]
pub(crate) enum Docker {}

#[cfg(not(unix))]
impl Docker {
    pub(crate) async fn open(_: Option<&str>) -> Option<Self> {
        None
    }

    pub(crate) fn host(&self) -> String {
        match *self {}
    }

    pub(crate) async fn started(&self) {
        match *self {}
    }

    pub(crate) async fn containers(
        &self,
    ) -> anyhow::Result<Vec<beep_proto::node::agent::RunningContainer>> {
        match *self {}
    }

    pub(crate) async fn stop(&self, _: &str) -> anyhow::Result<()> {
        match *self {}
    }
}

#[cfg(unix)]
mod unix {
    use anyhow::{Context, bail};
    use beep_proto::node::agent::RunningContainer;
    use bytes::Bytes;
    use http_body_util::{BodyExt, Empty, Full, Limited, combinators::BoxBody};
    use hyper::{
        Method, Request, Response, StatusCode, body::Incoming, header, service::service_fn,
    };
    use hyper_util::rt::TokioIo;
    use serde::Deserialize;
    use serde_json::Value;
    use sha2::{Digest, Sha256};
    use std::{
        convert::Infallible,
        fs,
        os::unix::fs::DirBuilderExt,
        path::{Path, PathBuf},
        sync::Arc,
        time::Duration,
    };
    use tokio::{
        net::{UnixListener, UnixStream},
        sync::Notify,
        task::JoinSet,
        time::timeout,
    };
    use tokio_util::task::AbortOnDropHandle;

    /// The label that names the session a container belongs to.
    pub(crate) const LABEL: &str = "beep.session";
    /// The largest container definition the socket reads to label it.
    const MAX_CREATE: usize = 4 << 20;
    /// The largest container list it reads from the daemon.
    const MAX_LIST: usize = 8 << 20;
    /// How long a list may take: a daemon that hangs must not hold up the
    /// session's processes.
    const LIST_WITHIN: Duration = Duration::from_secs(3);
    /// How long a stop may take: Docker gives a container ten seconds.
    const STOP_WITHIN: Duration = Duration::from_secs(15);

    type Body = BoxBody<Bytes, hyper::Error>;

    pub(crate) struct Docker {
        shared: Arc<Shared>,
        socket: PathBuf,
        _server: AbortOnDropHandle<()>,
    }

    struct Shared {
        /// The daemon's socket.
        upstream: PathBuf,
        /// The value of `LABEL` on this session's containers.
        token: String,
        /// Rung when a container of this session starts.
        started: Notify,
    }

    impl Docker {
        /// A socket for a new session, in front of the daemon that `host`
        /// names (the harness's `DOCKER_HOST`), else the node's
        /// `DOCKER_HOST`, else the current Docker context, else the default
        /// socket. `None` when that daemon is not on a Unix socket this node
        /// reaches.
        pub(crate) async fn open(host: Option<&str>) -> Option<Self> {
            let host = host.map(str::to_owned);
            let upstream = tokio::task::spawn_blocking(move || upstream(host.as_deref()))
                .await
                .ok()??;

            if let Err(e) = UnixStream::connect(&upstream).await {
                log::debug!("no Docker for sessions at {}: {e}", upstream.display());
                return None;
            }

            let dir = dirs::cache_dir()?.join("beep-node").join("docker");
            Self::listen(&dir, upstream)
                .inspect_err(|e| log::warn!("could not open a Docker socket for a session: {e:#}"))
                .ok()
        }

        /// Listens in `dir` and passes requests to the daemon at `upstream`.
        pub(crate) fn listen(dir: &Path, upstream: PathBuf) -> anyhow::Result<Self> {
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(dir)
                .with_context(|| format!("could not make {}", dir.display()))?;
            let token = format!("{:016x}", rand::random::<u64>());
            let socket = dir.join(format!("{token}.sock"));
            let listener = UnixListener::bind(&socket)
                .with_context(|| format!("could not listen on {}", socket.display()))?;
            let shared = Arc::new(Shared {
                upstream,
                token,
                started: Notify::new(),
            });
            let server = AbortOnDropHandle::new(tokio::spawn(serve(listener, shared.clone())));

            Ok(Self {
                shared,
                socket,
                _server: server,
            })
        }

        /// The session's `DOCKER_HOST`.
        pub(crate) fn host(&self) -> String {
            format!("unix://{}", self.socket.display())
        }

        /// Waits until one of the session's containers starts.
        pub(crate) async fn started(&self) {
            self.shared.started.notified().await;
        }

        /// The session's running containers.
        pub(crate) async fn containers(&self) -> anyhow::Result<Vec<RunningContainer>> {
            timeout(LIST_WITHIN, self.list())
                .await
                .map_err(|_| anyhow::anyhow!("Docker did not list its containers in time"))?
        }

        async fn list(&self) -> anyhow::Result<Vec<RunningContainer>> {
            let filters = format!(r#"{{"label":["{LABEL}={}"]}}"#, self.shared.token);
            let filters: String =
                url::form_urlencoded::byte_serialize(filters.as_bytes()).collect();
            let request = Request::get(format!("/containers/json?filters={filters}"))
                .header(header::HOST, "docker")
                .body(empty())?;
            let reply = ask(&self.shared.upstream, request).await?;
            let status = reply.status();
            let body = Limited::new(reply.into_body(), MAX_LIST)
                .collect()
                .await
                .map_err(|e| anyhow::anyhow!("could not read Docker's containers: {e}"))?
                .to_bytes();

            if !status.is_success() {
                bail!("Docker answered {status} for its containers");
            }

            let listed: Vec<Listed> =
                serde_json::from_slice(&body).context("Docker's containers do not read")?;

            Ok(listed.into_iter().map(RunningContainer::from).collect())
        }

        /// Stops container `id`, keeping it.
        pub(crate) async fn stop(&self, id: &str) -> anyhow::Result<()> {
            if id.is_empty() || !id.chars().all(|c| c.is_ascii_hexdigit()) {
                bail!("not a container id: {id}");
            }

            let request = Request::post(format!("/containers/{id}/stop"))
                .header(header::HOST, "docker")
                .body(empty())?;
            let status = timeout(STOP_WITHIN, ask(&self.shared.upstream, request))
                .await
                .map_err(|_| anyhow::anyhow!("Docker did not stop the container in time"))??
                .status();

            if status.is_success() || status == StatusCode::NOT_MODIFIED {
                return Ok(());
            }

            bail!("Docker answered {status} to stopping the container")
        }
    }

    impl Drop for Docker {
        fn drop(&mut self) {
            if let Err(e) = fs::remove_file(&self.socket) {
                log::debug!("could not remove {}: {e}", self.socket.display());
            }
        }
    }

    /// A container as `GET /containers/json` lists it.
    #[derive(Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct Listed {
        id: String,
        #[serde(default)]
        names: Vec<String>,
        #[serde(default)]
        image: String,
        #[serde(default)]
        created: u64,
        #[serde(default)]
        ports: Vec<Port>,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct Port {
        #[serde(default)]
        public_port: Option<u16>,
        #[serde(default, rename = "Type")]
        protocol: String,
    }

    impl From<Listed> for RunningContainer {
        fn from(listed: Listed) -> Self {
            let mut ports: Vec<u16> = listed
                .ports
                .iter()
                .filter(|port| port.protocol == "tcp")
                .filter_map(|port| port.public_port.filter(|port| *port > 0))
                .collect();
            ports.sort_unstable();
            ports.dedup();
            let name = listed
                .names
                .first()
                .map(|name| name.trim_start_matches('/').to_owned())
                .unwrap_or_default();

            Self {
                id: listed.id,
                name,
                image: listed.image,
                started_at: listed.created,
                ports,
                pages: Vec::new(),
            }
        }
    }

    /// The daemon's socket for `host`, or the node's own setting.
    fn upstream(harness: Option<&str>) -> Option<PathBuf> {
        let host = harness
            .map(str::to_owned)
            .or_else(|| std::env::var("DOCKER_HOST").ok())
            .filter(|host| !host.is_empty())
            .or_else(context_host);

        match host {
            Some(host) => host.strip_prefix("unix://").map(PathBuf::from),
            None => Some(PathBuf::from("/var/run/docker.sock")),
        }
    }

    /// The daemon the current Docker context names, unless it is `default`.
    fn context_host() -> Option<String> {
        let config = std::env::var_os("DOCKER_CONFIG")
            .map(PathBuf::from)
            .or_else(|| dirs::home_dir().map(|home| home.join(".docker")))?;
        let name = std::env::var("DOCKER_CONTEXT")
            .ok()
            .filter(|name| !name.is_empty())
            .or_else(|| {
                let text = fs::read(config.join("config.json")).ok()?;
                let config: Value = serde_json::from_slice(&text).ok()?;
                config.get("currentContext")?.as_str().map(str::to_owned)
            })
            .filter(|name| !name.is_empty() && name != "default")?;
        let meta = config
            .join("contexts")
            .join("meta")
            .join(hex::encode(Sha256::digest(name.as_bytes())))
            .join("meta.json");
        let meta: Value = serde_json::from_slice(&fs::read(meta).ok()?).ok()?;

        meta.pointer("/Endpoints/docker/Host")?
            .as_str()
            .map(str::to_owned)
    }

    async fn serve(listener: UnixListener, shared: Arc<Shared>) {
        // Dropped with the socket, which ends every connection still open.
        let mut connections = JoinSet::new();

        loop {
            tokio::select! {
                accepted = listener.accept() => match accepted {
                    Ok((stream, _)) => {
                        let shared = shared.clone();
                        connections.spawn(async move {
                            let service = service_fn(move |request| forward(request, shared.clone()));
                            let served = hyper::server::conn::http1::Builder::new()
                                .serve_connection(TokioIo::new(stream), service)
                                .with_upgrades()
                                .await;

                            if let Err(e) = served {
                                log::debug!("a session's Docker connection ended: {e}");
                            }
                        });
                    }
                    Err(e) => {
                        log::warn!("a session's Docker socket stopped: {e}");
                        return;
                    }
                },
                Some(_) = connections.join_next() => {}
            }
        }
    }

    /// The requests the socket looks at.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Call {
        Create,
        Start,
        Other,
    }

    impl Call {
        fn of(method: &Method, path: &str) -> Self {
            if method != Method::POST {
                return Self::Other;
            }

            let path = unversioned(path);

            if path == "/containers/create" {
                return Self::Create;
            }

            let action = path
                .strip_prefix("/containers/")
                .and_then(|rest| rest.split_once('/'))
                .map(|(_, action)| action);

            match action {
                Some("start" | "restart" | "unpause") => Self::Start,
                _ => Self::Other,
            }
        }
    }

    /// `path` without the API version the client puts first, such as `/v1.47`.
    fn unversioned(path: &str) -> &str {
        let Some(rest) = path.strip_prefix("/v") else {
            return path;
        };

        match rest.find('/') {
            Some(at) if at > 0 && rest[..at].chars().all(|c| c.is_ascii_digit() || c == '.') => {
                &rest[at..]
            }
            _ => path,
        }
    }

    /// Passes `request` to the daemon, labelling a container it creates, and
    /// joins the two for an attach or exec that takes over the connection.
    async fn forward(
        mut request: Request<Incoming>,
        shared: Arc<Shared>,
    ) -> Result<Response<Body>, Infallible> {
        let call = Call::of(request.method(), request.uri().path());
        let client = hyper::upgrade::on(&mut request);
        let (mut parts, body) = request.into_parts();

        let body = match call {
            Call::Create => match Limited::new(body, MAX_CREATE).collect().await {
                Ok(collected) => {
                    let bytes = collected.to_bytes();
                    let bytes = labelled(&bytes, &shared.token).map_or(bytes, Bytes::from);
                    parts.headers.remove(header::TRANSFER_ENCODING);
                    parts
                        .headers
                        .insert(header::CONTENT_LENGTH, bytes.len().into());
                    full(bytes)
                }
                Err(e) => return Ok(failure(StatusCode::BAD_REQUEST, &format!("{e}"))),
            },
            Call::Start | Call::Other => body.boxed(),
        };

        let mut reply = match ask(&shared.upstream, Request::from_parts(parts, body)).await {
            Ok(reply) => reply,
            Err(e) => {
                let message = format!("could not reach Docker: {e:#}");
                return Ok(failure(StatusCode::BAD_GATEWAY, &message));
            }
        };

        if reply.status() == StatusCode::SWITCHING_PROTOCOLS {
            let daemon = hyper::upgrade::on(&mut reply);
            // Ends when either side closes.
            tokio::spawn(async move {
                match tokio::try_join!(client, daemon) {
                    Ok((client, daemon)) => {
                        let (mut client, mut daemon) = (TokioIo::new(client), TokioIo::new(daemon));
                        let _ = tokio::io::copy_bidirectional(&mut client, &mut daemon).await;
                    }
                    Err(e) => log::debug!("a Docker attach did not take over: {e}"),
                }
            });
        }

        if call == Call::Start && reply.status().is_success() {
            shared.started.notify_one();
        }

        Ok(reply.map(BodyExt::boxed))
    }

    /// A container definition with the session's label added, or `None`
    /// when it does not read, which leaves it as the client sent it.
    pub(crate) fn labelled(body: &[u8], token: &str) -> Option<Vec<u8>> {
        let mut create: Value = serde_json::from_slice(body).ok()?;
        let labels = create
            .as_object_mut()?
            .entry("Labels")
            .or_insert(Value::Null);

        if labels.is_null() {
            *labels = Value::Object(serde_json::Map::new());
        }

        labels
            .as_object_mut()?
            .insert(LABEL.to_owned(), Value::from(token));

        serde_json::to_vec(&create).ok()
    }

    /// Sends `request` to the daemon on a connection of its own.
    async fn ask(upstream: &Path, request: Request<Body>) -> anyhow::Result<Response<Incoming>> {
        let stream = UnixStream::connect(upstream).await?;
        let (mut sender, connection) =
            hyper::client::conn::http1::handshake(TokioIo::new(stream)).await?;
        // Ends with the connection, after the reply's body or an attach.
        tokio::spawn(async move {
            if let Err(e) = connection.with_upgrades().await {
                log::debug!("a connection to Docker ended: {e}");
            }
        });

        Ok(sender.send_request(request).await?)
    }

    fn failure(status: StatusCode, message: &str) -> Response<Body> {
        let body = serde_json::json!({ "message": message }).to_string();
        let mut reply = Response::new(full(Bytes::from(body)));
        *reply.status_mut() = status;
        reply.headers_mut().insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("application/json"),
        );
        reply
    }

    fn full(bytes: Bytes) -> Body {
        Full::new(bytes).map_err(|never| match never {}).boxed()
    }

    fn empty() -> Body {
        Empty::new().map_err(|never| match never {}).boxed()
    }
}

#[cfg(all(test, unix))]
#[path = "../../tests/agents_processes_docker.rs"]
mod tests;
