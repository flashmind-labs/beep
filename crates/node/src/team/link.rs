//! `beep-team/1`: the link between the meshes of a team. A connection is
//! served only for a device on one of this mesh's rosters, or one that opens
//! with `Join` or, from a contact, `Invite`; anything else is closed. Every
//! message names its repo, and one about a team its sender is not on is
//! refused.

use super::{
    asks::{Asks, Sessions, not_served},
    board::{self, Boards},
    cards::Teammates,
    home::{Home, Invites},
    proof::{PROOF_CONTEXTS, ProofError},
    roster::{Refusal, Rosters},
};
use beep_mesh::TeamTicket;
use beep_projects::commands::OpenIssue;
use beep_proto::node::events::{TEAM_CARDS_TOPIC, TEAM_INVITES_TOPIC, Topic};
use beep_proto::watched::read_sent;
use beep_proto::{
    node::{LinkRx, LinkTx, link, tools::TeamInviteArgs},
    team::{ALPN, ForgeLogin, Member, Removed, TeamMsg, TeamRecord, TeamRepo},
};
use futures::future::BoxFuture;
use iroh::{
    Endpoint, EndpointAddr, EndpointId,
    endpoint::Connection,
    protocol::{AcceptError, ProtocolHandler},
};
use std::{
    collections::BTreeMap,
    fmt,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, SystemTime},
};
use tokio::{
    sync::{Notify, watch},
    time::{sleep, timeout},
};

/// How long a device gets to connect when dialed.
const DIAL: Duration = Duration::from_secs(5);
/// How long a peer gets to open its stream and send its first message.
pub(super) const READ: Duration = Duration::from_secs(10);
/// The largest first message: a stranger's `Join` or `Invite` is small.
const FIRST_FRAME: usize = 16 * 1024;
/// How long the forge gets to say whether a login may push.
const VOUCH_LIMIT: Duration = Duration::from_secs(20);
/// How often a removal goes again to the members it did not reach. Short in tests.
const RESEND: Duration = Duration::from_secs(if cfg!(test) { 1 } else { 10 * 60 });

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

    /// `repo`'s open issues.
    fn open_issues<'a>(
        &'a self,
        repo: &'a TeamRepo,
    ) -> BoxFuture<'a, Result<Vec<OpenIssue>, String>>;

    /// `repo`'s open issues assigned to `login`, every page.
    fn assigned_issues<'a>(
        &'a self,
        repo: &'a TeamRepo,
        login: &'a ForgeLogin,
    ) -> BoxFuture<'a, Result<Vec<OpenIssue>, String>>;

    /// The branches of `repo`'s pull requests, open or closed, that may come from `branch`:
    /// `branch` among them when one does.
    fn pull_heads<'a>(
        &'a self,
        repo: &'a TeamRepo,
        branch: &'a str,
    ) -> BoxFuture<'a, Result<Vec<String>, String>>;

    /// Marks commit `sha` of `repo` with the status `context`, as this
    /// machine's account: only one who may push can.
    fn stamp<'a>(
        &'a self,
        repo: &'a TeamRepo,
        sha: &'a str,
        context: &'a str,
    ) -> BoxFuture<'a, Result<(), String>>;

    /// The logins that marked commit `sha` of `repo` with a status whose
    /// context starts with `context`.
    fn stampers<'a>(
        &'a self,
        repo: &'a TeamRepo,
        sha: &'a str,
        context: &'a str,
    ) -> BoxFuture<'a, Result<Vec<String>, String>>;
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
    #[error("The team link failed: {0:#}")]
    Link(#[from] anyhow::Error),
}

/// What the link keeps of this mesh's teams: the rosters. The rest has its
/// own owner: `Asks` (a lock of its own), `Home` (the home task), `Served`
/// (the gateway), `Invites` and the handed queues (small locks).
#[derive(Default)]
pub(crate) struct Teams {
    pub(crate) rosters: Rosters,
}

/// What this device serves as its mesh's gateway, as the gateway loop
/// publishes it (`TeamLink::publish`): nobody else changes it.
#[derive(Default)]
pub(crate) struct Served {
    /// The machine of this mesh that acts for each team, as this device
    /// elected it.
    pub(crate) gateways: BTreeMap<TeamRepo, EndpointId>,
    /// The boards this device serves as its mesh's gateway.
    pub(crate) boards: Boards,
    /// The teammates' cards this device follows as its mesh's gateway.
    pub(crate) teammates: BTreeMap<TeamRepo, Teammates>,
}

/// What the asks read of the home and the served boards: see `TeamLink::seen`.
pub(super) struct Seen {
    pub(super) own: Option<EndpointId>,
    pub(super) serves: bool,
    pub(super) teammate_mesh: Option<EndpointId>,
    pub(super) on_board: bool,
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
    /// The asks this device's sessions sent, and those it keeps as gateway.
    asks: Mutex<Asks>,
    /// This mesh as the mesh says it, published by the home task.
    home: watch::Sender<Home>,
    /// What the gateway serves, published by the gateway loop.
    served: watch::Sender<Served>,
    /// Contacts' invites waiting for a desktop.
    invites: Mutex<Invites>,
    /// The pickup queues the gateways handed this device, for its gateway
    /// loop to keep.
    queues: Mutex<BTreeMap<TeamRepo, Vec<u64>>>,
    /// Where asks and their answers are handed, once the node runs sessions.
    sessions: OnceLock<Arc<dyn Sessions>>,
    /// Rings when a roster or an invite waits for a desktop.
    waiting: Notify,
}

impl TeamLink {
    pub(crate) fn new(endpoint: Endpoint, forge: Arc<dyn TeamForge>) -> Self {
        let inner = Inner {
            endpoint,
            forge,
            teams: Mutex::new(Teams::default()),
            asks: Mutex::default(),
            home: watch::channel(Home::default()).0,
            served: watch::channel(Served::default()).0,
            invites: Mutex::default(),
            queues: Mutex::default(),
            sessions: OnceLock::new(),
            waiting: Notify::new(),
        };

        Self {
            inner: Arc::new(inner),
        }
    }

    /// This device's id.
    pub(crate) fn me(&self) -> EndpointId {
        self.inner.endpoint.id()
    }

    pub(crate) fn handler(&self) -> TeamAlpn {
        TeamAlpn(self.clone())
    }

    #[cfg(test)]
    pub(crate) fn endpoint(&self) -> &Endpoint {
        &self.inner.endpoint
    }

    pub(crate) fn forge(&self) -> &dyn TeamForge {
        self.inner.forge.as_ref()
    }

    /// Hands asks and their answers to `sessions` from now on.
    pub(crate) fn serve_sessions(&self, sessions: Arc<dyn Sessions>) {
        if self.inner.sessions.set(sessions).is_err() {
            log::warn!("the team link already serves sessions");
        }
    }

    pub(super) fn sessions(&self) -> Option<&dyn Sessions> {
        self.inner.sessions.get().map(Arc::as_ref)
    }

    /// Reads or changes what the link knows. Keep `f` short: it holds the lock.
    pub(crate) fn with<R>(&self, f: impl FnOnce(&mut Teams) -> R) -> R {
        f(&mut crate::lock(&self.inner.teams))
    }

    /// Reads or changes the asks. Keep `f` short: it holds the lock.
    pub(crate) fn asks<R>(&self, f: impl FnOnce(&mut Asks) -> R) -> R {
        f(&mut crate::lock(&self.inner.asks))
    }

    /// Reads or changes the rosters and the asks together, for what checks one
    /// against the other. Rosters are locked first, always.
    pub(crate) fn both<R>(&self, f: impl FnOnce(&mut Teams, &mut Asks) -> R) -> R {
        let mut teams = crate::lock(&self.inner.teams);

        f(&mut teams, &mut crate::lock(&self.inner.asks))
    }

    /// `f` of this mesh as the mesh says it. Not inside another watch read.
    pub(crate) fn home<R>(&self, f: impl FnOnce(&Home) -> R) -> R {
        read_sent(&self.inner.home, f)
    }

    /// Wakes when the mesh's teams, contacts or projects were read again.
    pub(super) fn home_changes(&self) -> watch::Receiver<Home> {
        self.inner.home.subscribe()
    }

    /// Changes what the home task published, for a test.
    #[cfg(test)]
    pub(crate) fn edit_home(&self, f: impl FnOnce(&mut Home)) {
        self.inner.home.send_modify(f);
    }

    /// Publishes what the home task read of the mesh: the only writer.
    pub(super) fn set_home(&self, home: Home) {
        self.inner.home.send_replace(home);
    }

    /// `f` of what the gateway serves. Not inside another watch read.
    pub(crate) fn served<R>(&self, f: impl FnOnce(&Served) -> R) -> R {
        read_sent(&self.inner.served, f)
    }

    /// What the asks read of the home and of what the gateway serves for
    /// `repo`, taken before they lock the rosters: whether this device
    /// serves its board, which mesh the teammates' card `teammate` is on,
    /// and whether the card `on_board` is on the board.
    pub(super) fn seen(&self, repo: &TeamRepo, teammate: Option<&str>, on_board: Option<&str>) -> Seen {
        let own = self.home(|home| home.mesh);

        self.served(|served| Seen {
            own,
            serves: served.boards.serves(repo),
            teammate_mesh: teammate
                .and_then(|card| served.teammates.get(repo)?.mesh_of(card)),
            on_board: on_board.is_some_and(|card| {
                let cards = served.boards.cards(repo).unwrap_or_default();
                cards.iter().any(|on| on.id == card)
            }),
        })
    }

    /// Changes what the gateway serves (publishes), for its readers: the gateway loop
    /// is the only caller. Keep `f` short: it holds the value.
    pub(crate) fn publish(&self, f: impl FnOnce(&mut Served)) {
        let me = self.me();
        let mut acting = Vec::new();
        self.inner.served.send_modify(|served| {
            f(served);
            acting = served
                .gateways
                .iter()
                .filter(|(_, gateway)| **gateway == me)
                .map(|(repo, _)| repo.clone())
                .collect();
        });

        // The boards and the teammates' cards are what this device serves.
        for repo in &acting {
            self.cards_changed(repo);
        }
    }

    /// Says `repo`'s cards changed: the `team-cards` topic of its project,
    /// which a reader of it takes the cards from. Only the machine that acts
    /// for the team has them; the others' readers get nothing.
    pub(crate) fn cards_changed(&self, repo: &TeamRepo) {
        let project = self.home(|home| home.projects.get(repo).map(|held| held.name.clone()));

        if let Some(project) = project {
            crate::events::system().changed(Topic::keyed(TEAM_CARDS_TOPIC, project));
        }
    }

    /// The invites waiting for a desktop now.
    pub(crate) fn invites_waiting(&self) -> Vec<TeamInviteArgs> {
        crate::lock(&self.inner.invites).current(SystemTime::now())
    }

    /// Keeps a contact's invite for a desktop to show, and says the
    /// `team-invites` topic changed.
    pub(crate) fn keep_invite(&self, invite: TeamInviteArgs) -> Result<(), String> {
        crate::lock(&self.inner.invites).keep(invite, SystemTime::now())?;
        crate::events::system().changed(Topic::new(TEAM_INVITES_TOPIC));

        Ok(())
    }

    /// Forgets `invite`, and says the `team-invites` topic changed.
    pub(crate) fn drop_invite(&self, invite: &TeamInviteArgs) {
        crate::lock(&self.inner.invites).remove(invite);
        crate::events::system().changed(Topic::new(TEAM_INVITES_TOPIC));
    }

    /// Keeps the queue a team's gateway handed this device.
    pub(crate) fn hand_queue(&self, repo: TeamRepo, issues: Vec<u64>) {
        crate::lock(&self.inner.queues).insert(repo, issues);
    }

    /// Takes the queues handed since the last call.
    pub(super) fn take_queues(&self) -> BTreeMap<TeamRepo, Vec<u64>> {
        std::mem::take(&mut *crate::lock(&self.inner.queues))
    }

    /// Forgets all that the link knew, once the mesh and the tasks that
    /// owned it are gone.
    pub(crate) fn reset(&self) {
        *crate::lock(&self.inner.teams) = Teams::default();
        *crate::lock(&self.inner.asks) = Asks::default();
        *crate::lock(&self.inner.invites) = Invites::default();
        crate::events::system().changed(Topic::new(TEAM_INVITES_TOPIC));
        crate::lock(&self.inner.queues).clear();
        self.inner.home.send_replace(Home::default());
        self.inner.served.send_replace(Served::default());
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
        let mesh = self.home(|home| home.mesh);
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

    /// Sends `msg` to each of `members`, in the background. A removal goes
    /// again every `RESEND` to those it did not reach, for as long as they
    /// still owe it: a member offline then would keep the removed one on.
    pub(super) fn tell(&self, members: Vec<Member>, msg: TeamMsg) {
        let link = self.clone();

        tokio::spawn(async move {
            let mut unreached = members;

            loop {
                let mut missed = Vec::new();

                for member in unreached {
                    if let Err(e) = link.send(&member, &msg).await {
                        log::info!("a team message did not reach {}: {e}", member.login);
                        missed.push(member);
                    }
                }

                if missed.is_empty() || !matches!(msg, TeamMsg::Remove { .. }) {
                    return;
                }

                sleep(RESEND).await;
                unreached = link.with(|teams| {
                    let owed = missed.into_iter().filter(|member| owes(teams, member, &msg));
                    owed.collect()
                });
            }
        });
    }

    /// Sends `msg` to the first of `member`'s devices that answers; an error
    /// unless it took it.
    pub(super) async fn send(&self, member: &Member, msg: &TeamMsg) -> Result<(), TeamError> {
        match self.offer(&member.devices, msg).await? {
            None => Ok(()),
            Some(reason) => Err(TeamError::Refused(reason)),
        }
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

    /// Whether `peer` is on `repo`'s team, which this device serves the board of.
    fn serves(&self, peer: EndpointId, repo: &TeamRepo) -> bool {
        self.with(|teams| teams.rosters.allows(peer, repo))
            && self.served(|served| served.boards.serves(repo))
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
                && self.serves(peer, repo)
            {
                return board::serve(self, peer, repo.clone(), tx, rx).await;
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

        let own = self.home(|home| home.mesh);
        let applied = match self.with(|teams| teams.rosters.apply(peer, own, msg)) {
            Ok(applied) => applied,
            Err(refusal) => return refused(refusal.to_string()),
        };

        if applied.changed.is_some() {
            self.wake();
        }

        if let TeamMsg::Roster { repo, .. } = msg {
            for newcomer in applied.newcomers {
                if let Err(reason) = self.vouch(repo, &newcomer).await {
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
            TeamMsg::Subscribe { repo } => refused(not_served(repo)),
            TeamMsg::Ask { .. } => self.asked(peer, msg).await,
            TeamMsg::Reply { id, text } => self.answered(peer, id, text).await,
            _ => None,
        }
    }

    /// Whether the forge lets `newcomer`, a member another mesh admitted, on
    /// `repo`'s team: only a definite yes does. When the forge cannot tell,
    /// as Forgejo for anyone but a repository's admins, the status they put
    /// on their proof commit does: only those who may push mark a commit. The
    /// reason to give when not, which says nothing of this machine; the
    /// sender offers the roster again later.
    pub(super) async fn vouch(&self, repo: &TeamRepo, newcomer: &Member) -> Result<(), String> {
        let login = &newcomer.login;
        let vouched = async {
            match (self.may_push(repo, login).await, &newcomer.proof) {
                (Some(pushes), _) => Some(pushes),
                (None, Some(sha)) => {
                    let marked = self.marked(repo, sha, PROOF_CONTEXTS, login).await;
                    marked.filter(|marked| *marked)
                }
                (None, None) => None,
            }
        };

        match timeout(VOUCH_LIMIT, vouched).await {
            Ok(Some(true)) => Ok(()),
            Ok(Some(false)) => Err(format!("{login} is not a collaborator on {repo}")),
            Ok(None) | Err(_) => Err(format!("Could not check {login} on {repo}'s forge")),
        }
    }

    /// Whether the forge says `login` may push to `repo`; `None`, logged,
    /// when it cannot tell within `VOUCH_LIMIT`.
    pub(super) async fn may_push(&self, repo: &TeamRepo, login: &ForgeLogin) -> Option<bool> {
        match timeout(VOUCH_LIMIT, self.forge().collaborator(repo, login)).await {
            Ok(Ok(pushes)) => Some(pushes),
            Ok(Err(e)) => {
                log::info!("could not check {login} on {repo}: {e}");
                None
            }
            Err(_) => {
                log::info!("the forge took too long to check {login} on {repo}");
                None
            }
        }
    }

    /// Keeps a contact's invite for a desktop to show; refuses anyone else's,
    /// and invites that do not read or are for another repository.
    fn invited(&self, peer: EndpointId, repo: TeamRepo, ticket: String) -> Option<TeamMsg> {
        let from = self.home(|home| home.contacts.get(&peer).copied());
        let kept = from
            .ok_or_else(|| Refusal::Stranger.to_string())
            .and_then(|from| {
                let read = TeamTicket::parse(&ticket).map_err(|_| "The invite does not read")?;

                if read.repo != repo {
                    return Err("The invite is for another repository".to_owned());
                }

                self.keep_invite(TeamInviteArgs { from, repo, ticket })
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

/// Whether `member` still owes `msg`, a removal: they are on its team here
/// or are the one it removed, and its login is removed here and held by no
/// member. A readmission ends it, so that it never takes off the member let
/// back in.
fn owes(teams: &Teams, member: &Member, msg: &TeamMsg) -> bool {
    let TeamMsg::Remove { repo, login } = msg else {
        return false;
    };

    teams.rosters.get(repo).is_some_and(|record| {
        let removed = |gone: &Removed| gone.login == *login;
        let on = record.members.iter().any(|on| on.mesh == member.mesh);
        let them = record.removed.iter().any(|gone| removed(gone) && gone.mesh == member.mesh);
        let back = record.members.iter().any(|on| on.login == *login);

        (on || them) && record.removed.iter().any(removed) && !back
    })
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
