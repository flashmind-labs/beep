//! Serves one node connection: the hello, the claim on a token and the request loop.

use super::{
    CLOSE_UNAVAILABLE, MAX_TOOL_CALLS, NODE_ALPN, NODE_ALPN_NAMED, Online, push_sync, stats,
};
use crate::{
    account::user::User,
    app::App,
    protocol::{
        link::MAX_NAME,
        node::{
            CLOSE_SETUP_REFUSED, Down, LinkRx, LinkTx, MAX_FRAME, NodeHello, NodeSetup, Up,
            agent::SESSIONS_PROTOCOL, link_on,
        },
        setup::SetupToken,
    },
    push::CLOSE_REVOKED,
    store::{Claim, ClaimNode, ClaimedNode, NewNode},
    tools::machines::{
        Machines,
        mcp::{self, Calls},
    },
};
use anyhow::{Context, Result, bail};
use iroh::{
    EndpointId,
    endpoint::{Connection, VarInt},
    protocol::{AcceptError, ProtocolHandler, RouterBuilder},
};
use std::{fmt, sync::Arc};
use tokio::{sync::Notify, task::JoinSet, time::timeout};
use tokio_util::task::AbortOnDropHandle;

#[derive(Clone)]
pub struct NodeHandler {
    app: Arc<App>,
}

impl fmt::Debug for NodeHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NodeHandler").finish()
    }
}

/// Serves `handler` on `beep-node/2` and on `beep-node/1`, in that order: the
/// Router keeps it, and a node that offers both settles on the first.
pub fn accept_nodes(router: RouterBuilder, handler: NodeHandler) -> RouterBuilder {
    router
        .accept(NODE_ALPN, handler.clone())
        .accept(NODE_ALPN_NAMED, handler)
}

impl NodeHandler {
    /// `server` is the key of the endpoint the handler serves on.
    pub fn new(app: Arc<App>, server: EndpointId) -> Self {
        if app.nodes.server.set(server).is_err() && app.nodes.server() != Some(server) {
            log::warn!("node handler made again for another server key");
        }
        Self { app }
    }
}

impl ProtocolHandler for NodeHandler {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let endpoint = connection.remote_id();
        // An unlinked key gets only a small hello read, so a stranger cannot make us buffer megabytes.
        let linked = match self.app.store.linked_sign_in(&endpoint).await {
            Ok(sign_in) => sign_in.is_some(),
            Err(e) => {
                log::debug!("node {endpoint} looked up as unlinked: {e:#}");
                false
            }
        };
        let (tx, rx, mut hello) = match read_hello(&connection, linked).await {
            Ok(opened) => opened,
            Err(e) => {
                log::debug!("node {endpoint} sent no hello: {e:#}");
                connection.close(VarInt::from_u32(CLOSE_UNAVAILABLE), b"no hello");
                return Ok(());
            }
        };
        let mut claimed = None;
        if let Some(setup) = hello.setup.take() {
            match claim(&self.app, &endpoint, &setup, &hello.name).await {
                Ok(node) => {
                    hello.name = node.name.clone();
                    claimed = Some(node);
                }
                Err(Refused::Setup(reason)) => {
                    log::info!("node setup refused; key={endpoint} reason={reason:?}");
                    connection.close(VarInt::from_u32(CLOSE_SETUP_REFUSED), reason.as_bytes());
                    return Ok(());
                }
                Err(Refused::Failed(e)) => {
                    log::warn!("node setup failed: {e:#}");
                    connection.close(VarInt::from_u32(CLOSE_UNAVAILABLE), b"unavailable");
                    return Ok(());
                }
            }
        }
        let user = match self.app.sign_in_linked(&endpoint).await {
            Ok(Some(signed_in)) => signed_in.user,
            Ok(None) => {
                connection.close(VarInt::from_u32(CLOSE_REVOKED), b"not linked");
                return Ok(());
            }
            Err(e) => {
                log::warn!("node sign in failed: {e:#}");
                connection.close(VarInt::from_u32(CLOSE_UNAVAILABLE), b"unavailable");
                return Ok(());
            }
        };
        let control = Control {
            tx,
            rx,
            hello,
            claimed,
        };
        if let Err(e) = serve(&self.app, user, &connection, control).await {
            log::debug!("node {endpoint} left: {e:#}");
        }
        self.app.nodes.remove(&endpoint, &connection);
        Ok(())
    }
}

/// The control stream once its hello is read.
pub(super) struct Control {
    tx: LinkTx,
    rx: LinkRx,
    hello: NodeHello,
    /// The token the hello claimed, when it carried one.
    claimed: Option<ClaimedNode>,
}

/// The longest hello read from a key nobody linked.
pub(super) const MAX_UNLINKED_HELLO: usize = 16 * 1024;

pub(super) async fn read_hello(
    connection: &Connection,
    linked: bool,
) -> Result<(LinkTx, LinkRx, NodeHello)> {
    let (send, recv) = timeout(crate::api::transport::READ_TIMEOUT, connection.accept_bi())
        .await
        .context("no control stream")??;
    let (tx, mut rx) = link_on(connection, send, recv);
    if !linked {
        rx.set_max_frame(MAX_UNLINKED_HELLO);
    }
    let hello: NodeHello = timeout(crate::api::transport::READ_TIMEOUT, rx.recv())
        .await
        .context("no hello")??
        .context("closed before its hello")?;
    rx.set_max_frame(MAX_FRAME);
    Ok((tx, rx, hello))
}

/// Why a setup token did not link the node.
pub(super) enum Refused {
    /// In words for the user, sent as the close reason.
    Setup(String),
    Failed(anyhow::Error),
}

impl From<anyhow::Error> for Refused {
    fn from(e: anyhow::Error) -> Self {
        Self::Failed(e)
    }
}

/// Claims the hello's setup token for `endpoint`, named `fallback` if it gives no name.
pub(super) async fn claim(
    app: &Arc<App>,
    endpoint: &EndpointId,
    setup: &NodeSetup,
    fallback: &str,
) -> Result<ClaimedNode, Refused> {
    let refuse = |reason: &str| Err(Refused::Setup(reason.to_owned()));
    let Ok(token) = SetupToken::parse(&setup.token) else {
        return refuse("That is not a setup token. Copy the whole command from Beep again.");
    };
    if app.nodes.server() != Some(token.server) {
        return refuse("This setup token is for another server.");
    }
    let named = setup.name.as_deref().map(str::trim);
    if named.is_some_and(|name| name.is_empty() || name.len() > MAX_NAME) {
        return refuse("The machine's name is empty or too long.");
    }
    let fallback = match fallback.trim() {
        name if name.is_empty() || name.len() > MAX_NAME => "Machine",
        name => name,
    };
    let hash = token.hash();
    let claim = ClaimNode {
        hash: &hash,
        endpoint: *endpoint,
        named,
        fallback,
    };
    match app.store.claim_node_token(claim).await? {
        Claim::Linked(node) => {
            log::info!(
                "node linked by setup token; user={} key={endpoint}",
                node.account.id
            );
            Ok(node)
        }
        Claim::Unknown => refuse("This setup token is unknown here. Make a new one in Beep."),
        Claim::Expired => refuse("This setup token expired. Make a new one in Beep."),
        Claim::Used => refuse("This setup token was already used. Make a new one in Beep."),
        Claim::OtherAccount => refuse(
            "This machine's key belongs to another Beep account. On the machine, delete \
             beep-node's key folder (beep-node in its config folder, or the --dir you gave) \
             and run setup again.",
        ),
    }
}

/// Reads the control stream until the node goes away.
pub(super) async fn serve(
    app: &Arc<App>,
    user: Arc<User>,
    connection: &Connection,
    control: Control,
) -> Result<()> {
    let endpoint = connection.remote_id();
    let Control {
        mut tx,
        mut rx,
        hello,
        claimed,
    } = control;
    user.node_connected(NewNode {
        endpoint,
        name: &hello.name,
        version: &hello.version,
    })
    .await?;
    if let Some(claimed) = claimed {
        user.changed("");
        tx.send(&Down::Linked {
            account: claimed.account_name,
            name: claimed.name,
        })
        .await?;
    }
    tx.send(&Down::Welcome { tools: true }).await?;
    log::info!(
        "node connected; user={} name={:?} version={} offers={:?} workspace={:?}",
        user.id(),
        hello.name,
        hello.version,
        hello.offers,
        hello.workspace
    );
    // Only a node that takes credentials gets them; the first set goes at once.
    let credentials = hello.credentials.then(|| Arc::new(Notify::new()));
    if let Some(changed) = &credentials {
        changed.notify_one();
    }
    // The same for the harness settings.
    let agent_settings = hello.agent_settings.then(|| Arc::new(Notify::new()));
    if let Some(changed) = &agent_settings {
        changed.notify_one();
    }
    let stats = Arc::new(stats::StatsSlot::default());
    let online = Online {
        user: user.id().to_owned(),
        name: hello.name,
        offers: hello.offers,
        workspace: hello.workspace,
        checkouts: hello.checkouts,
        branch_checkouts: hello.branch_checkouts,
        connection: connection.clone(),
        app: Arc::downgrade(app),
        shell: Arc::default(),
        agents: Arc::default(),
        desktop: Arc::default(),
        stats: stats.clone(),
        credentials: credentials.clone(),
        agent_settings: agent_settings.clone(),
        usage_refresh: Arc::default(),
    };
    app.nodes.insert(endpoint, online.clone());
    let _usage = online.offers(SESSIONS_PROTOCOL).then(|| {
        let (user, online) = (user.clone(), online.clone());
        AbortOnDropHandle::new(tokio::spawn(async move {
            if let Err(e) = online.keep_usage(&user).await {
                log::debug!("agent usage not followed; node={endpoint} error={e:#}");
            }
        }))
    });
    let _alerts = online.offers(SESSIONS_PROTOCOL).then(|| {
        let (user, online) = (user.clone(), online.clone());
        AbortOnDropHandle::new(tokio::spawn(async move {
            if let Err(e) = online.keep_alerts(&user).await {
                log::debug!("agent alerts not followed; node={endpoint} error={e:#}");
            }
        }))
    });
    // Webhooks left on repositories of removed projects, while no node was
    // online to remove them.
    let _webhooks = online.forwards_webhooks().then(|| {
        let (user, online) = (user.clone(), online.clone());
        AbortOnDropHandle::new(tokio::spawn(async move {
            if let Err(e) = crate::projects::repo_watch::sweep(&user, &online).await {
                log::warn!("old webhooks not removed; node={endpoint} error={e:#}");
            }
        }))
    });
    let _images = online.offers(SESSIONS_PROTOCOL).then(|| {
        let user = user.clone();
        AbortOnDropHandle::new(tokio::spawn(async move {
            if let Err(e) = online.keep_images(&user).await {
                log::debug!("agent images not followed; node={endpoint} error={e:#}");
            }
        }))
    });
    app.nodes.follows.reconnected(app, &endpoint.to_string());
    // The project workers on the node report again, or are marked gone.
    app.projects.reconnected(app, &user, endpoint);
    // Checkouts of the user's projects it holds become their folders there.
    app.checkouts.check(app, &user, &endpoint.to_string());
    // Sends the node its synced projects, and its peers how it is reached now.
    let _push = {
        let (app, user) = (app.clone(), user.clone());
        AbortOnDropHandle::new(tokio::spawn(async move {
            if let Err(e) = push_sync(&app.nodes, &user, Some(endpoint)).await {
                log::warn!("sync list not sent; node={endpoint} error={e:#}");
            }
        }))
    };
    // Agent calls through the Beep MCP server, each on a stream the node opens.
    let mut calls = JoinSet::new();
    loop {
        tokio::select! {
            stream = connection.accept_bi() => {
                let Ok((send, recv)) = stream else {
                    return Ok(());
                };
                if calls.len() >= MAX_TOOL_CALLS {
                    log::warn!("refused an agent tool call; node={endpoint}: {MAX_TOOL_CALLS} already run");
                    continue;
                }
                let calls_from = Calls {
                    machines: Machines {
                        app: app.clone(),
                        user: user.clone(),
                    },
                    caller: endpoint.to_string(),
                };
                let (tx, rx) = link_on(connection, send, recv);
                calls.spawn(mcp::serve(calls_from, tx, rx));
            }
            Some(ended) = calls.join_next() => match ended {
                Ok(Ok(())) => {}
                Ok(Err(e)) => log::debug!("agent tool call failed; node={endpoint} error={e:#}"),
                Err(e) => log::warn!("agent tool call task failed; node={endpoint} error={e}"),
            },
            message = rx.recv::<Up>() => match message {
                Ok(Some(Up::Env(env))) => {
                    // A report that fails to save is not worth dropping the node.
                    if let Err(e) = user.save_node_env(&endpoint, &env).await {
                        log::warn!("node report not saved; node={endpoint} error={e:#}");
                    }
                }
                Ok(Some(Up::Stats(report))) => stats.set(report),
                Ok(Some(Up::Traffic(report))) => {
                    // Counters are cumulative: the next report makes up for this one.
                    if let Err(e) = user.save_node_traffic(&endpoint, &report).await {
                        log::warn!("node traffic not saved; node={endpoint} error={e:#}");
                    }
                }
                Ok(Some(other)) => log::debug!("node sent something unhandled: {other:?}"),
                Ok(None) => return Ok(()),
                // A message from a newer node this server cannot read yet.
                Err(e) if connection.close_reason().is_none() => {
                    log::debug!("node message skipped: {e:#}")
                }
                Err(e) => bail!(e),
            },
            () = changed(credentials.as_deref()) => {
                match user.machine_credentials(&app.credential_key).await {
                    Ok(set) => tx.send(&Down::Credentials(set)).await?,
                    // The node keeps what it had; the next change tries again.
                    Err(e) => log::warn!("credentials not sent; node={endpoint} error={e:#}"),
                }
            }
            () = changed(agent_settings.as_deref()) => {
                match user.agent_settings(&app.credential_key, &endpoint.to_string()).await {
                    Ok(settings) => tx.send(&Down::AgentSettings(settings)).await?,
                    // The node keeps what it had; the next change tries again.
                    Err(e) => log::warn!("harness settings not sent; node={endpoint} error={e:#}"),
                }
            }
        }
    }
}

/// Finishes when `notify` is woken; never without one.
pub(super) async fn changed(notify: Option<&Notify>) {
    match notify {
        Some(notify) => notify.notified().await,
        None => std::future::pending().await,
    }
}
