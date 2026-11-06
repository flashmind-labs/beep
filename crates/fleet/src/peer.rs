//! Reaching a machine of the mesh: the client the desktop hands in, a shell and the list of found
//! folders over it, and what the machine says about itself.

use crate::error::{FleetError, Result};
use async_trait::async_trait;
use beep_mesh::{Mesh, PeerClient};
use beep_projects::{Clip, Shell, ShellOutput, for_machine, imports::machine::FoundFolders};
use beep_proto::node::{
    EnvReport,
    agent::{SESSIONS_PROTOCOL, SessionFolder, SessionsReply, SessionsRequest},
    shell::{self, Event, Request},
    status::{self, Report},
};
use iroh::EndpointId;
use std::time::Duration;
use tokio::time::timeout;

/// How long a machine has to say what it offers and what it is.
const PROBE: Duration = Duration::from_secs(3);
/// Slack past a command's own limit, for the machine to say it timed out.
const SHELL_SLACK: Duration = Duration::from_secs(10);

/// The desktop's hold on the mesh: the mesh itself, and a client for each member.
#[async_trait]
pub trait Peers: Send + Sync {
    /// The mesh, waiting for it to open.
    async fn mesh(&self) -> Result<Mesh>;

    /// This Mac's own id.
    fn me(&self) -> EndpointId;

    /// A client for `machine`, which may be this Mac's own node.
    async fn client(&self, machine: EndpointId) -> Result<PeerClient>;

    /// What `machine` last said about itself, kept from before: its probe is slow, and an
    /// offline machine says nothing now. Nothing is kept by default.
    fn last_env(&self, _machine: EndpointId) -> Option<EnvReport> {
        None
    }
}

/// A shell on one machine, over its `shell/1` protocol. Scripts are POSIX sh, and on Windows
/// they run in the sh of Git for Windows. A script may carry a token, so nothing here logs it.
pub(crate) struct PeerShell {
    client: PeerClient,
    windows: bool,
}

impl PeerShell {
    pub(crate) fn new(client: PeerClient, windows: bool) -> Self {
        Self { client, windows }
    }
}

#[async_trait]
impl Shell for PeerShell {
    async fn run(&self, script: &str, limit: Duration) -> anyhow::Result<ShellOutput> {
        let run = async {
            let (mut tx, mut rx) = self.client.open(shell::PROTOCOL).await?;
            let timeout_secs = u32::try_from(limit.as_secs().max(1)).unwrap_or(u32::MAX);
            tx.send(&Request::Run {
                command: for_machine(script, self.windows).into_owned(),
                timeout_secs,
            })
            .await?;
            let mut clip = Clip::default();
            loop {
                match rx.recv::<Event>().await? {
                    Some(Event::Output { text }) => clip.push(&text),
                    Some(Event::Done {
                        exit_code,
                        timed_out,
                        ..
                    }) => {
                        return Ok(ShellOutput {
                            exit_code,
                            timed_out,
                            output: clip.finish(),
                        });
                    }
                    Some(_) => {}
                    None => anyhow::bail!("the machine closed the shell"),
                }
            }
        };
        timeout(limit + SHELL_SLACK, run)
            .await
            .map_err(|_| anyhow::anyhow!("the machine did not finish the command"))?
    }
}

/// A machine's coding agent sessions, for the folders they worked in.
pub(crate) struct PeerFound {
    client: PeerClient,
}

impl PeerFound {
    pub(crate) fn new(client: PeerClient) -> Self {
        Self { client }
    }
}

#[async_trait]
impl FoundFolders for PeerFound {
    async fn found(&self, limit: u32) -> anyhow::Result<Vec<SessionFolder>> {
        let reply: SessionsReply = self
            .client
            .call(SESSIONS_PROTOCOL, &SessionsRequest::Folders { limit })
            .await?;
        match reply {
            SessionsReply::Folders { folders } => Ok(folders),
            SessionsReply::Error { message } => anyhow::bail!(message),
            _ => anyhow::bail!("the machine answered something else"),
        }
    }
}

/// What a machine said about itself on `status/1`.
#[derive(Default)]
pub(crate) struct Probe {
    pub(crate) offers: Vec<String>,
    pub(crate) env: Option<EnvReport>,
}

impl Probe {
    pub(crate) fn offers(&self, protocol: &str) -> bool {
        self.offers.iter().any(|offered| offered == protocol)
    }
}

/// Reads the machine's `Offers` and `Env`. A machine that is slow to say gets what it said so far.
pub(crate) async fn probe(client: &PeerClient) -> Result<Probe> {
    let mut said = Probe::default();
    let mut offered = false;
    let read = async {
        // Held open: finishing our half would end the stream.
        let (_tx, mut rx) = client.open(status::PROTOCOL).await?;
        while !(offered && said.env.is_some()) {
            match rx.recv::<Report>().await.map_err(FleetError::Other)? {
                Some(Report::Offers(offers)) => {
                    said.offers = offers;
                    offered = true;
                }
                Some(Report::Env(env)) => said.env = Some(env),
                Some(_) => {}
                None => break,
            }
        }
        Ok::<(), FleetError>(())
    };
    match timeout(PROBE, read).await {
        Ok(Ok(())) | Err(_) => {}
        Ok(Err(e)) => return Err(e),
    }
    Ok(said)
}
