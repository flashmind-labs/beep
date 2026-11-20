//! `beep-team/1`: the link between the meshes of a team. A connection is
//! served only for a device on one of this mesh's rosters, or one that opens
//! with `Join` or, from a contact, `Invite`; anything else is closed. Every
//! message names its repo, and one about a team its sender is not on is
//! refused.

use super::{
    cards::IssueTitles,
    gateway::{self, Boards},
    home::{Home, Invites},
    proof::ProofError,
    roster::{Refusal, Rosters},
};
use beep_proto::{
    node::{LinkRx, LinkTx, link, tools::TeamInviteArgs},
    team::{ALPN, ForgeLogin, Member, TeamMsg, TeamRecord, TeamRepo},
};
use beep_mesh::TeamTicket;
use futures::future::BoxFuture;
use iroh::{
    Endpoint, EndpointAddr, EndpointId,
    endpoint::Connection,
    protocol::{AcceptError, ProtocolHandler},
};
use std::{
    fmt,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime},
};
use tokio::{sync::Notify, time::timeout};

/// How long a device gets to connect when dialed.
const DIAL: Duration = Duration::from_secs(5);
/// How long a peer gets to open its stream and send its first message.
const READ: Duration = Duration::from_secs(10);
/// The largest first message: a stranger's `Join` or `Invite` is small.
const FIRST_FRAME: usize = 16 * 1024;
/// How long the forge gets to say whether a login may push.
const VOUCH_LIMIT: Duration = Duration::from_secs(20);

/// What the forge says about a team's people. In tests it answers without one.
pub(crate) trait TeamForge: Send + Sync + 'static {
    /// Whether `login` may push to `repo`.
    fn collaborator<'a>(
        &'a self,
        repo: &'a TeamRepo,
        login: &'a ForgeLogin,
    ) -> BoxFuture<'a, Result<bool, String>>;

    /// The account this machine is signed in with on `repo`'s host.
    fn login<'a>(&'a self, repo: &'a TeamRepo) -> BoxFuture<'a, Result<ForgeLogin, String>>;

    /// The titles of `repo`'s open issues.
    fn issue_titles<'a>(&'a self, repo: &'a TeamRepo) -> BoxFuture<'a, Result<IssueTitles, String>>;
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum TeamError {
    #[error("{0}")]
    Refused(String),
    #[error("No device of the team answered")]
    Unreachable,
    #[error("Joining took longer than a minute")]
    TimedOut,
    #[error("This device is in no mesh")]
    NoMesh,
    #[error("Import {0} first")]
    NotImported(TeamRepo),
    #[error(transparent)]
    Proof(#[from] ProofError),
    #[error("{0}")]
    Forge(String),
    #[error("The team link failed: {0:#}")]
    Link(#[from] anyhow::Error),
}

/// What the link knows of this mesh's teams.
#[derive(Default)]
pub(crate) struct Teams {
    pub(crate) rosters: Rosters,
    pub(crate) home: Home,
    /// Contacts' invites to other teams, for a desktop to show.
    pub(crate) invites: Invites,
    /// The boards this device serves as its mesh's gateway.
    pub(crate) boards: Boards,
}

/// The team link of this device, shared by its handler, its dialing and the
/// task that follows the mesh.
#[derive(Clone)]
pub(crate) struct TeamLink {
    inner: Arc<Inner>,
}

struct Inner {
    endpoint: Endpoint,
    forge: Arc<dyn TeamForge>,
    teams: Mutex<Teams>,
    /// Rings when a roster or an invite waits for a desktop.
    waiting: Notify,
}

impl TeamLink {
    pub(crate) fn new(endpoint: Endpoint, forge: Arc<dyn TeamForge>) -> Self {
        let inner = Inner {
            endpoint,
            forge,
            teams: Mutex::new(Teams::default()),
            waiting: Notify::new(),
        };

        Self {
            inner: Arc::new(inner),
        }
    }

    pub(crate) fn handler(&self) -> TeamAlpn {
        TeamAlpn(self.clone())
    }

    #[cfg(test)]
    pub(crate) fn endpoint(&self) -> &Endpoint {
        &self.inner.endpoint
    }

    pub(super) fn forge(&self) -> &dyn TeamForge {
        self.inner.forge.as_ref()
    }

    /// Reads or changes what the link knows. Keep `f` short: it holds the lock.
    pub(crate) fn with<R>(&self, f: impl FnOnce(&mut Teams) -> R) -> R {
        f(&mut crate::lock(&self.inner.teams))
    }

    /// Returns once a roster or an invite waits for a desktop.
    pub(crate) async fn waiting(&self) {
        self.inner.waiting.notified().await;
    }

    pub(super) fn wake(&self) {
        self.inner.waiting.notify_one();
    }

    /// A stream to the first of `addrs` that answers, each tried for 5 s.
    /// Dialed directly: the mesh's own connections refuse other meshes.
    pub(crate) async fn dial(&self, addrs: &[EndpointAddr]) -> Result<(LinkTx, LinkRx), TeamError> {
        for addr in addrs {
            let opening = async {
                let connection = self.inner.endpoint.connect(addr.clone(), ALPN).await?;
                anyhow::Ok(connection.open_bi().await?)
            };

            match timeout(DIAL, opening).await {
                Ok(Ok((send, recv))) => return Ok(link(send, recv)),
                Ok(Err(e)) => log::debug!("team link to {}: {e:#}", addr.id.fmt_short()),
                Err(_) => log::debug!("team link to {}: no answer", addr.id.fmt_short()),
            }
        }

        Err(TeamError::Unreachable)
    }

    /// Sends `record`'s roster to the members of other meshes, but not to
    /// `except`'s, in the background.
    pub(super) fn spread(&self, record: TeamRecord, except: EndpointId) {
        let mesh = self.with(|teams| teams.home.mesh);
        let roster = TeamMsg::Roster {
            repo: record.repo.clone(),
            members: record.members.clone(),
        };
        let others = record
            .members
            .into_iter()
            .filter(|member| Some(member.mesh) != mesh && member.mesh != except)
            .collect();

        self.tell(others, roster);
    }

    /// Sends `msg` to each of `members`, in the background.
    pub(super) fn tell(&self, members: Vec<Member>, msg: TeamMsg) {
        let link = self.clone();

        tokio::spawn(async move {
            for member in members {
                let sent = async {
                    let (mut tx, _rx) = link.dial(&member.devices).await?;
                    tx.send(&msg).await?;
                    finish(tx).await?;
                    Ok::<_, TeamError>(())
                };

                if let Err(e) = sent.await {
                    log::info!("a team message did not reach {}: {e}", member.login);
                }
            }
        });
    }

    async fn serve(&self, connection: &Connection) -> Result<(), TeamError> {
        let peer = connection.remote_id();
        let (send, recv) = timeout(READ, connection.accept_bi())
            .await
            .map_err(|_| TeamError::Refused("no stream".to_owned()))?
            .map_err(anyhow::Error::from)?;
        let (mut tx, mut rx) = link(send, recv);

        // A stranger's first message is small; a member's may be a whole roster.
        if !self.with(|teams| teams.rosters.knows(peer)) {
            rx.set_max_frame(FIRST_FRAME);
        }

        let first = timeout(READ, rx.recv::<TeamMsg>())
            .await
            .map_err(|_| TeamError::Refused("no message".to_owned()))??;

        match first {
            None => Ok(()),
            Some(TeamMsg::Join {
                secret,
                repo,
                login,
                mesh,
                devices,
            }) => {
                let join = super::join::Join {
                    secret,
                    repo,
                    login,
                    mesh,
                    devices,
                };
                self.admit(peer, join, tx, rx).await
            }
            Some(TeamMsg::Invite { repo, ticket }) => {
                if let Some(refused) = self.invited(peer, repo, ticket) {
                    tx.send(&refused).await?;
                }
                finish(tx).await?;
                Ok(())
            }
            Some(first) if self.with(|teams| teams.rosters.knows(peer)) => {
                self.serve_member(peer, first, tx, rx).await
            }
            Some(_) => {
                connection.close(1u32.into(), b"not on a team");
                Ok(())
            }
        }
    }

    /// Answers a member's messages until they finish the stream.
    async fn serve_member(
        &self,
        peer: EndpointId,
        first: TeamMsg,
        mut tx: LinkTx,
        mut rx: LinkRx,
    ) -> Result<(), TeamError> {
        let mut next = Some(first);

        while let Some(msg) = next {
            if let TeamMsg::Subscribe { repo } = &msg
                && self.with(|teams| teams.rosters.allows(peer, repo) && teams.boards.serves(repo))
            {
                return gateway::serve(self, peer, repo.clone(), tx, rx).await;
            }

            if let Some(answer) = self.answer(peer, &msg).await {
                tx.send(&answer).await?;
            }
            next = rx.recv().await?;
        }

        finish(tx).await?;
        Ok(())
    }

    /// The answer to one message from a member's device, if any. A roster's
    /// newcomers are added once the forge says they may push.
    pub(super) async fn answer(&self, peer: EndpointId, msg: &TeamMsg) -> Option<TeamMsg> {
        let refused = |reason: String| Some(TeamMsg::Refused { reason });

        // An invite is to a team the contact is not on yet.
        if let TeamMsg::Invite { repo, ticket } = msg {
            return self.invited(peer, repo.clone(), ticket.clone());
        }

        let own = self.with(|teams| teams.home.mesh);
        let applied = match self.with(|teams| teams.rosters.apply(peer, own, msg)) {
            Ok(applied) => applied,
            Err(refusal) => return refused(refusal.to_string()),
        };

        if applied.changed.is_some() {
            self.wake();
        }

        if let TeamMsg::Roster { repo, .. } = msg {
            for newcomer in applied.newcomers {
                if let Err(reason) = self.vouch(repo, &newcomer.login).await {
                    return refused(reason);
                }

                match self.with(|teams| teams.rosters.add(repo, peer, own, newcomer)) {
                    Ok(Some(_)) => self.wake(),
                    Ok(None) => {}
                    Err(refusal) => return refused(refusal.to_string()),
                }
            }
        }

        match msg {
            TeamMsg::Join { .. } => refused("A join opens its own link".to_owned()),
            TeamMsg::Subscribe { repo } | TeamMsg::Ask { repo, .. } => {
                refused(format!("This device does not serve {repo}'s board"))
            }
            _ => None,
        }
    }

    /// Whether the forge says `login` may push to `repo`; the reason to give
    /// when not, which says nothing of this machine.
    pub(super) async fn vouch(&self, repo: &TeamRepo, login: &ForgeLogin) -> Result<(), String> {
        let answer = timeout(VOUCH_LIMIT, self.forge().collaborator(repo, login)).await;

        match answer {
            Ok(Ok(true)) => Ok(()),
            Ok(Ok(false)) => Err(format!("{login} is not a collaborator on {repo}")),
            Ok(Err(e)) => {
                log::warn!("could not check {login} on {repo}: {e}");
                Err("Could not check access on the forge".to_owned())
            }
            Err(_) => {
                log::warn!("the forge took too long to check {login} on {repo}");
                Err("Could not check access on the forge".to_owned())
            }
        }
    }

    /// Keeps a contact's invite for a desktop to show; refuses anyone else's,
    /// and invites that do not read or are for another repository.
    fn invited(&self, peer: EndpointId, repo: TeamRepo, ticket: String) -> Option<TeamMsg> {
        let kept = self.with(|teams| {
            let from = *teams
                .home
                .contacts
                .get(&peer)
                .ok_or_else(|| Refusal::Stranger.to_string())?;
            let read = TeamTicket::parse(&ticket).map_err(|_| "The invite does not read")?;

            if read.repo != repo {
                return Err("The invite is for another repository".to_owned());
            }

            let invite = TeamInviteArgs { from, repo, ticket };
            teams.invites.keep(invite, SystemTime::now())
        });

        match kept {
            Ok(()) => {
                self.wake();
                None
            }
            Err(reason) => Some(TeamMsg::Refused { reason }),
        }
    }
}

/// Finishes `tx` once the peer read it, or after `READ`: a peer that holds
/// the link without reading does not hold this side with it.
pub(super) async fn finish(tx: LinkTx) -> Result<(), TeamError> {
    timeout(READ, tx.finish())
        .await
        .map_err(|_| TeamError::Refused("the peer stopped reading".to_owned()))??;

    Ok(())
}

/// Serves `beep-team/1`.
pub struct TeamAlpn(TeamLink);

impl fmt::Debug for TeamAlpn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TeamAlpn")
    }
}

impl ProtocolHandler for TeamAlpn {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        if let Err(e) = self.0.serve(&connection).await {
            log::debug!(
                "a team link from {} ended: {e}",
                connection.remote_id().fmt_short()
            );
        }

        Ok(())
    }
}

#[cfg(test)]
#[path = "../tests/team_link.rs"]
mod tests;
