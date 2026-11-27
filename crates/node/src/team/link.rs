//! `beep-team/1`: the link between the meshes of a team. A connection is
//! served only for a device on one of this mesh's rosters, or one that opens
//! with `Join` or, from a contact, `Invite`; anything else is closed. Every
//! message names its repo, and one about a team its sender is not on is
//! refused.

mod asks_out;
pub(super) mod board_io;
pub(crate) mod home_io;

pub(crate) use super::served::Served;

use super::{
    served::ServedBoards,
    asks::{Answer, Asks, not_served},
    home::{Home, Invites, TeamForge},
    proof::{PROOF_CONTEXTS, ProofError},
    roster::{Refusal, Rosters},
};
use beep_mesh::TeamTicket;
use beep_proto::node::events::{TEAM_CARDS_TOPIC, TEAM_INVITES_TOPIC, Topic};
use beep_proto::watched::read_sent;
use beep_proto::{
    node::{LinkRx, LinkTx, link, tools::TeamInviteArgs},
    team::{ALPN, ForgeLogin, Member, Removed, TeamMsg, TeamRecord, TeamRepo},
};
use futures::future::join_all;
use iroh::{Endpoint, EndpointAddr, EndpointId};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime},
};
use tokio::{
    sync::{Notify, watch},
    time::{sleep, timeout},
};

/// How long a device gets to connect when dialed.
const DIAL: Duration = Duration::from_secs(5);
/// How long a peer gets to open its stream and send its first message.
pub(crate) const READ: Duration = Duration::from_secs(10);
/// The largest first message: a stranger's `Join` or `Invite` is small.
pub(crate) const FIRST_FRAME: usize = 16 * 1024;
/// How long the forge gets to say whether a login may push.
const VOUCH_LIMIT: Duration = Duration::from_secs(20);
/// How often a removal goes again to the members it did not reach. Short in tests.
const RESEND: Duration = Duration::from_secs(if cfg!(test) { 1 } else { 10 * 60 });

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
    served: ServedBoards,
    /// Contacts' invites waiting for a desktop.
    invites: Mutex<Invites>,
    /// The pickup queues the gateways handed this device, for its gateway
    /// loop to keep.
    queues: Mutex<BTreeMap<TeamRepo, Vec<u64>>>,
    /// Rings when a roster or an invite waits for a desktop.
    waiting: Notify,
    /// Rings when an invite or a card was added, which may expire sooner than the next to.
    expiring: Notify,
    /// Rises whenever a roster is edited, for the gateway to elect again.
    rostered: watch::Sender<u64>,
}

impl TeamLink {
    pub(crate) fn new(endpoint: Endpoint, forge: Arc<dyn TeamForge>) -> Self {
        let inner = Inner {
            endpoint,
            forge,
            teams: Mutex::new(Teams::default()),
            asks: Mutex::default(),
            home: watch::channel(Home::default()).0,
            served: ServedBoards::default(),
            invites: Mutex::default(),
            queues: Mutex::default(),
            waiting: Notify::new(),
            expiring: Notify::new(),
            rostered: watch::channel(0).0,
        };

        Self {
            inner: Arc::new(inner),
        }
    }

    /// This device's id.
    pub(crate) fn me(&self) -> EndpointId {
        self.inner.endpoint.id()
    }

    #[cfg(test)]
    pub(crate) fn endpoint(&self) -> &Endpoint {
        &self.inner.endpoint
    }

    pub(crate) fn forge(&self) -> &dyn TeamForge {
        self.inner.forge.as_ref()
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
        self.inner.served.read(f)
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
        let acting = self.inner.served.publish(self.me(), f);

        // The boards and the teammates' cards are what this device serves. A
        // team it stopped acting for rings too: its readers now get nothing.
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

        self.inner.expiring.notify_one();
    }

    /// When the oldest invite or card expires by age.
    pub(crate) fn next_expiry(&self) -> Option<SystemTime> {
        let invites = crate::lock(&self.inner.invites).due();
        let asks = crate::lock(&self.inner.asks).due();

        invites.into_iter().chain(asks).min()
    }

    /// Resolves when an invite or a card was added.
    pub(crate) async fn expiry_changed(&self) {
        self.inner.expiring.notified().await;
    }

    /// Drops what expired by age and says the topics of the invites and the cards changed.
    pub(crate) fn expire_now(&self) {
        let now = SystemTime::now();
        let repos: Vec<TeamRepo> = self.asks(|asks| asks.cards.keys().cloned().collect());
        crate::lock(&self.inner.invites).expire(now);
        self.asks(|asks| asks.expire(now));
        crate::events::system().changed(Topic::new(TEAM_INVITES_TOPIC));

        for repo in &repos {
            self.cards_changed(repo);
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
        self.inner.expiring.notify_one();
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
        self.inner.served.clear();
    }

    /// Returns once a roster or an invite waits for a desktop.
    pub(crate) async fn waiting(&self) {
        self.inner.waiting.notified().await;
    }

    pub(super) fn wake(&self) {
        self.inner.waiting.notify_one();
        self.roster_edited();
    }

    /// Says a roster changed.
    pub(super) fn roster_edited(&self) {
        self.inner.rostered.send_modify(|edits| *edits += 1);
    }

    /// Rises whenever a roster is edited.
    pub(super) fn roster_edits(&self) -> watch::Receiver<u64> {
        self.inner.rostered.subscribe()
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
                let sent = join_all(unreached.iter().map(|member| link.send(member, &msg))).await;
                let missed: Vec<Member> = unreached
                    .into_iter()
                    .zip(sent)
                    .filter_map(|(member, sent)| match sent {
                        Ok(()) => None,
                        Err(e) => {
                            log::info!("a team message did not reach {}: {e}", member.login);
                            Some(member)
                        }
                    })
                    .collect();

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

    /// Admits the stranger whose first message is `join`, a `TeamMsg::Join`,
    /// on its own link. Anything else is not a join and ends the link.
    pub(crate) async fn admit_join(
        &self,
        peer: EndpointId,
        join: TeamMsg,
        tx: LinkTx,
        rx: LinkRx,
    ) -> Result<(), TeamError> {
        let TeamMsg::Join {
            secret,
            repo,
            login,
            mesh,
            devices,
        } = join
        else {
            return Ok(());
        };
        let join = super::join::Join {
            secret,
            repo,
            login,
            mesh,
            devices,
        };

        self.admit(peer, join, tx, rx).await
    }

    /// Whether `peer` is on `repo`'s team, which this device serves the board of.
    pub(crate) fn serves(&self, peer: EndpointId, repo: &TeamRepo) -> bool {
        self.with(|teams| teams.rosters.allows(peer, repo))
            && self.served(|served| served.boards.serves(repo))
    }

    /// The answer to one message from a member's device: what the link says
    /// back, if anything, or a text for a session to deliver first. A
    /// roster's newcomers are added once the forge says they may push.
    pub(crate) async fn answer(&self, peer: EndpointId, msg: &TeamMsg) -> Answer {
        let refused = |reason: String| Answer::Send(Some(TeamMsg::Refused { reason }));

        // An invite is to a team the contact is not on yet.
        if let TeamMsg::Invite { repo, ticket } = msg {
            return Answer::Send(self.invited(peer, repo.clone(), ticket.clone()));
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
            TeamMsg::Ask { .. } => self.asked(peer, msg),
            TeamMsg::Reply { id, text } => self.answered(peer, id, text),
            _ => Answer::Send(None),
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
    pub(crate) fn invited(&self, peer: EndpointId, repo: TeamRepo, ticket: String) -> Option<TeamMsg> {
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
pub(crate) async fn finish(tx: LinkTx) -> Result<(), TeamError> {
    timeout(READ, tx.finish())
        .await
        .map_err(|_| TeamError::Refused("the peer stopped reading".to_owned()))??;

    Ok(())
}

#[cfg(test)]
#[path = "../tests/team_link.rs"]
mod tests;
