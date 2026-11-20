//! The gateway: the one machine of this mesh that acts for a team project,
//! elected among the mesh's machines with its checkout as the CI watcher is
//! (`machine::elect`). It serves this mesh's cards to the teammates who
//! subscribe, follows each teammate's board for this mesh's apps on the
//! topic `team.<repo>` (`topic`), and checks once a day that every member
//! may still push.

use super::{
    cards::{Facts, IssueTitles, Teammates, card_of, changes, settle},
    home::Project,
    link::{TeamError, TeamLink, finish},
};
use crate::{
    agents::mcp::children::{Listed, in_the_mesh},
    link::Offers,
    machine::{
        ci::{Ci, forge::command},
        elect::{ask_reach, elect, gather, online},
    },
};
use beep_mesh::Mesh;
use beep_proto::{
    SessionId,
    node::{LinkRx, LinkTx, events::Topic, tools::ForgeReach},
    team::{Card, ForgeLogin, Lane, Member, TeamMsg, TeamRepo},
};
use futures::future::join_all;
use iroh::{EndpointAddr, EndpointId};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    time::{Duration, SystemTime},
};
use tokio::{
    select,
    sync::{broadcast, broadcast::error::RecvError, mpsc},
    time::{Instant, MissedTickBehavior, interval, sleep, timeout},
};
use tokio_util::task::AbortOnDropHandle;

/// How often the boards are made again and the roles elected.
const TICK: Duration = Duration::from_secs(10);
/// How often the issue titles are read while a team session works, and
/// otherwise; the machines are asked how they reach the hosts every `QUIET`.
const BUSY: Duration = Duration::from_secs(60);
const QUIET: Duration = Duration::from_secs(10 * 60);
/// How often every member is checked on the forge.
const UPKEEP: Duration = Duration::from_secs(24 * 60 * 60);
/// How long a forge call or a branch read may take.
const FORGE_LIMIT: Duration = Duration::from_secs(30);
const GIT_LIMIT: Duration = Duration::from_secs(5);
/// How long after a teammate's board drops it is dialed again.
const REDIAL: Duration = Duration::from_secs(60);
/// The largest message a teammate's gateway may send.
const MAX_FRAME: usize = 1024 * 1024;
/// Changes kept for a slow subscriber before it gets the whole board again.
const CHANGES: usize = 64;

/// The topic this mesh's apps read `repo`'s teammates' cards on.
pub(crate) fn topic(repo: &TeamRepo) -> Topic {
    Topic::keyed("team", repo.to_string())
}

/// The machine that acts for `repo`, as `me` sees it with the online `peers`.
pub(crate) fn elected(
    project: &Project,
    repo: &TeamRepo,
    me: EndpointId,
    ci: &Ci,
    peers: &BTreeMap<EndpointId, ForgeReach>,
) -> Option<EndpointId> {
    let machines = project.devices.iter().map(|device| device.id).collect();

    elect(&machines, me, ci.reach_of(&repo.host), peers, &repo.host)
}

/// Acts for the team projects this node is elected for, until the mesh is gone.
pub(crate) async fn run(mesh: Mesh, me: EndpointId, link: TeamLink, offers: Offers) {
    let (heard, mut hearing) = mpsc::channel(CHANGES);
    let mut updates = mesh.updates();
    let mut tick = interval(TICK);
    tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut gateway = Gateway {
        mesh,
        me,
        link,
        offers,
        heard,
        peers: BTreeMap::new(),
        asked: None,
        held: BTreeSet::new(),
        titles: BTreeMap::new(),
        branches: HashMap::new(),
        checked: BTreeMap::new(),
        follows: BTreeMap::new(),
        teammates: BTreeMap::new(),
    };

    loop {
        select! {
            _ = tick.tick() => gateway.step().await,
            Some(followed) = hearing.recv() => gateway.heard(followed),
            changed = updates.changed() => if changed.is_err() {
                return;
            },
        }
    }
}

/// The boards this device serves, for the teammates subscribed to them.
#[derive(Debug, Default)]
pub(crate) struct Boards(BTreeMap<TeamRepo, Board>);

#[derive(Debug)]
struct Board {
    cards: Vec<Card>,
    changes: broadcast::Sender<(Card, bool)>,
}

impl Boards {
    /// Serves `repo`'s board with `cards`, telling its subscribers what changed.
    pub(crate) fn set(&mut self, repo: &TeamRepo, cards: Vec<Card>) {
        let board = self.0.entry(repo.clone()).or_insert_with(|| Board {
            cards: Vec::new(),
            changes: broadcast::channel(CHANGES).0,
        });

        for change in changes(&board.cards, &cards) {
            let _ = board.changes.send(change);
        }

        board.cards = cards;
    }

    /// Serves only `held`'s boards: the others' subscribers are let go.
    fn keep(&mut self, held: &BTreeSet<TeamRepo>) {
        self.0.retain(|repo, _| held.contains(repo));
    }

    pub(crate) fn serves(&self, repo: &TeamRepo) -> bool {
        self.0.contains_key(repo)
    }

    fn cards(&self, repo: &TeamRepo) -> Option<Vec<Card>> {
        Some(self.0.get(repo)?.cards.clone())
    }

    fn watched(&self, repo: &TeamRepo) -> bool {
        self.0
            .get(repo)
            .is_some_and(|board| board.changes.receiver_count() > 0)
    }
}

/// Serves `repo`'s board to `peer`, a member's device: the whole board, then
/// each change, until the peer finishes the stream or is no longer on the
/// team, or the board moves to another machine. What else the peer sends
/// is answered as the link answers it.
pub(super) async fn serve(
    link: &TeamLink,
    peer: EndpointId,
    repo: TeamRepo,
    mut tx: LinkTx,
    mut rx: LinkRx,
) -> Result<(), TeamError> {
    let watched = link.with(|teams| {
        let board = teams.boards.0.get(&repo)?;
        Some((board.cards.clone(), board.changes.subscribe()))
    });
    let Some((cards, mut changes)) = watched else {
        return finish(tx).await;
    };
    tx.send(&TeamMsg::Board {
        repo: repo.clone(),
        cards,
    })
    .await?;

    loop {
        select! {
            change = changes.recv() => {
                let msg = match change {
                    _ if !link.with(|teams| teams.rosters.allows(peer, &repo)) => break,
                    Ok((card, gone)) => TeamMsg::BoardChange { repo: repo.clone(), card, gone },
                    Err(RecvError::Lagged(_)) => match link.with(|teams| teams.boards.cards(&repo)) {
                        Some(cards) => TeamMsg::Board { repo: repo.clone(), cards },
                        None => break,
                    },
                    Err(RecvError::Closed) => break,
                };
                tx.send(&msg).await?;
            }
            msg = rx.recv::<TeamMsg>() => match msg? {
                None => break,
                Some(msg) => {
                    if let Some(answer) = link.answer(peer, &msg).await {
                        tx.send(&answer).await?;
                    }
                }
            },
        }
    }

    finish(tx).await
}

/// A team this machine acts for.
struct Team {
    repo: TeamRepo,
    /// The mesh's name for the project.
    project: String,
    /// This mesh's login on it.
    login: ForgeLogin,
    /// The members of other meshes.
    others: Vec<Member>,
}

/// What a teammate's gateway said of their board.
enum Heard {
    Board(Vec<Card>),
    Change(Card, bool),
    Gone,
}

struct Followed {
    repo: TeamRepo,
    mesh: EndpointId,
    heard: Heard,
}

struct Gateway {
    mesh: Mesh,
    me: EndpointId,
    link: TeamLink,
    offers: Offers,
    heard: mpsc::Sender<Followed>,
    /// What the online machines said of their reach, and when they were
    /// asked and which were online then.
    peers: BTreeMap<EndpointId, ForgeReach>,
    asked: Option<(Instant, BTreeSet<EndpointId>)>,
    held: BTreeSet<TeamRepo>,
    /// Each held repository's open issue titles, and when they were read.
    titles: BTreeMap<TeamRepo, (IssueTitles, Instant)>,
    /// The branch of each session here, read once.
    branches: HashMap<String, Option<String>>,
    /// When each held team's members were last checked.
    checked: BTreeMap<TeamRepo, Instant>,
    /// The follow of each teammate's board, by repository and their mesh.
    follows: BTreeMap<(TeamRepo, EndpointId), (Member, AbortOnDropHandle<()>)>,
    teammates: BTreeMap<TeamRepo, Teammates>,
}

impl Gateway {
    async fn step(&mut self) {
        let (teams, home) = self.link.with(|link| {
            let own = link.home.mesh;
            let teams: Vec<Team> = link
                .home
                .projects
                .iter()
                .filter_map(|(repo, project)| {
                    let record = link.rosters.get(repo)?;
                    let (mine, others) = record
                        .members
                        .iter()
                        .cloned()
                        .partition::<Vec<_>, _>(|member| Some(member.mesh) == own);
                    Some(Team {
                        repo: repo.clone(),
                        project: project.name.clone(),
                        login: mine.into_iter().next()?.login,
                        others,
                    })
                })
                .collect();
            (teams, link.home.clone())
        });

        self.ask_peers(&teams, &home.projects).await;

        let online = online(&self.mesh);
        let peers: BTreeMap<EndpointId, ForgeReach> = self
            .peers
            .iter()
            .filter(|(peer, _)| online.contains(*peer))
            .map(|(peer, reach)| (*peer, reach.clone()))
            .collect();
        let ci = &self.offers.agents.ci;
        let me = self.me;
        let teams: Vec<Team> = teams
            .into_iter()
            .filter(|team| {
                let project = &home.projects[&team.repo];
                elected(project, &team.repo, me, ci, &peers) == Some(me)
            })
            .collect();
        self.held = teams.iter().map(|team| team.repo.clone()).collect();
        let held = &self.held;
        self.titles.retain(|repo, _| held.contains(repo));
        self.checked.retain(|repo, _| held.contains(repo));
        self.link.with(|link| {
            link.boards.keep(held);

            for repo in held {
                if !link.boards.serves(repo) {
                    link.boards.set(repo, Vec::new());
                }
            }
        });

        self.follow_teammates(&teams);
        self.serve_cards(&teams).await;
        self.upkeep(&teams).await;
    }

    /// Asks the online machines with a team project how they reach the
    /// hosts, every `QUIET` and when one comes or goes.
    async fn ask_peers(&mut self, teams: &[Team], projects: &BTreeMap<TeamRepo, Project>) {
        let online = online(&self.mesh);
        let due = self
            .asked
            .as_ref()
            .is_none_or(|(at, then)| at.elapsed() >= QUIET || *then != online);

        if !due {
            return;
        }

        let asked: BTreeSet<EndpointId> = teams
            .iter()
            .flat_map(|team| &projects[&team.repo].devices)
            .map(|device| device.id)
            .filter(|machine| online.contains(machine))
            .collect();
        self.asked = Some((Instant::now(), online));
        let mesh = &self.mesh;
        self.peers = gather("team gateway", asked, |peer| ask_reach(mesh, peer)).await;
    }

    /// Makes the cards of every held team with a subscriber.
    async fn serve_cards(&mut self, teams: &[Team]) {
        let watched: Vec<&Team> = teams
            .iter()
            .filter(|team| self.link.with(|link| link.boards.watched(&team.repo)))
            .collect();

        if watched.is_empty() {
            return;
        }

        let listed = in_the_mesh(&self.offers.agents, Some(&self.mesh), &SessionId::default()).await;
        self.read_branches(&listed).await;

        for team in watched {
            let titles = self.read_titles(&team.repo).await;
            let now = SystemTime::now();
            let facts = Facts {
                project: &team.project,
                login: &team.login,
                linked: &titles,
                now,
            };
            let fresh: Vec<Card> = listed
                .iter()
                .filter_map(|listed| {
                    let mut view = listed.view.clone();
                    view.machine = listed.machine.clone();
                    let branch = self.branches.get(&view.id).cloned().flatten();
                    card_of(&view, &facts, branch.as_deref())
                })
                .collect();

            self.link.with(|link| {
                let old = link.boards.cards(&team.repo).unwrap_or_default();
                link.boards.set(&team.repo, settle(&old, fresh, now));
            });
        }
    }

    /// Reads the branch of each session here not read yet.
    async fn read_branches(&mut self, listed: &[Listed]) {
        self.branches
            .retain(|id, _| listed.iter().any(|listed| listed.view.id == *id));
        let unread: Vec<(String, String)> = listed
            .iter()
            .filter(|listed| !self.branches.contains_key(&listed.view.id))
            .filter_map(|listed| Some((listed.view.id.clone(), listed.folder.clone()?)))
            .collect();
        let read = join_all(unread.iter().map(|(_, folder)| branch_of(folder))).await;

        self.branches
            .extend(unread.into_iter().map(|(id, _)| id).zip(read));
    }

    /// `repo`'s open issue titles: read again every `BUSY` while a session
    /// on its board works, every `QUIET` otherwise.
    async fn read_titles(&mut self, repo: &TeamRepo) -> IssueTitles {
        let working = self.link.with(|link| {
            let cards = link.boards.cards(repo).unwrap_or_default();
            cards.iter().any(|card| card.lane == Lane::Working {})
        });
        let wait = if working { BUSY } else { QUIET };

        if let Some((titles, at)) = self.titles.get(repo)
            && at.elapsed() < wait
        {
            return titles.clone();
        }

        let read = timeout(FORGE_LIMIT, self.link.forge().issue_titles(repo)).await;
        let titles = match read {
            Ok(Ok(titles)) => titles,
            Ok(Err(e)) => {
                log::info!("the issues of {repo} did not read: {e}");
                self.titles.get(repo).map(|(titles, _)| titles.clone()).unwrap_or_default()
            }
            Err(_) => {
                log::info!("the issues of {repo} took too long to read");
                self.titles.get(repo).map(|(titles, _)| titles.clone()).unwrap_or_default()
            }
        };
        self.titles
            .insert(repo.clone(), (titles.clone(), Instant::now()));

        titles
    }

    /// Follows the board of every teammate of the held teams, and stops
    /// following those who left, moved or are no longer held.
    fn follow_teammates(&mut self, teams: &[Team]) {
        let wanted: BTreeMap<(TeamRepo, EndpointId), Member> = teams
            .iter()
            .flat_map(|team| {
                let others = team.others.iter();
                others.map(|member| ((team.repo.clone(), member.mesh), member.clone()))
            })
            .collect();
        let stopped: Vec<(TeamRepo, EndpointId)> = self
            .follows
            .iter()
            .filter(|(key, (member, _))| wanted.get(*key) != Some(member))
            .map(|(key, _)| key.clone())
            .collect();

        for (repo, mesh) in stopped {
            self.follows.remove(&(repo.clone(), mesh));
            self.teammates.entry(repo.clone()).or_default().gone(mesh);
            self.publish(&repo);
        }

        self.teammates.retain(|repo, _| self.held.contains(repo));

        for (key, member) in wanted {
            if self.follows.contains_key(&key) {
                continue;
            }

            let task = follow(self.link.clone(), key.0.clone(), member.clone(), self.heard.clone());
            self.follows
                .insert(key, (member, AbortOnDropHandle::new(tokio::spawn(task))));
        }
    }

    /// Takes what a follow heard, and hands the teammates' cards to this
    /// mesh's apps.
    fn heard(&mut self, followed: Followed) {
        let Followed { repo, mesh, heard } = followed;
        let Some((member, _)) = self.follows.get(&(repo.clone(), mesh)) else {
            return;
        };
        let teammates = self.teammates.entry(repo.clone()).or_default();

        match heard {
            Heard::Board(cards) => teammates.board(mesh, &member.login, cards),
            Heard::Change(card, gone) => teammates.change(mesh, &member.login, card, gone),
            Heard::Gone => teammates.gone(mesh),
        }

        self.publish(&repo);
    }

    fn publish(&self, repo: &TeamRepo) {
        let cards = self
            .teammates
            .get(repo)
            .map(Teammates::cards)
            .unwrap_or_default();
        let published = rmp_serde::to_vec(&cards)
            .map_err(anyhow::Error::from)
            .and_then(|bytes| self.offers.agents.events.publish(topic(repo), bytes));

        if let Err(e) = published {
            log::warn!("the teammates' cards of {repo} were not published: {e:#}");
        }
    }

    /// Once a day for each held team, asks the forge whether each member of
    /// another mesh may still push; one who may not is removed, here and on
    /// every other member's side. A failed check waits for the next day.
    async fn upkeep(&mut self, teams: &[Team]) {
        for team in teams {
            if self.checked.get(&team.repo).is_some_and(|at| at.elapsed() < UPKEEP) {
                continue;
            }

            self.checked.insert(team.repo.clone(), Instant::now());

            for member in &team.others {
                let asked = self.link.forge().collaborator(&team.repo, &member.login);

                match timeout(FORGE_LIMIT, asked).await {
                    Ok(Ok(true)) => {}
                    Ok(Ok(false)) => self.remove(team, &member.login),
                    Ok(Err(e)) => log::info!("could not check {} on {}: {e}", member.login, team.repo),
                    Err(_) => log::info!("the forge took too long to check {} on {}", member.login, team.repo),
                }
            }
        }
    }

    /// Removes `login` from `team`, as a member's `Remove` does, and tells
    /// every other member.
    fn remove(&self, team: &Team, login: &ForgeLogin) {
        log::info!("{login} may no longer push to {}: removing them from the team", team.repo);
        let remove = TeamMsg::Remove {
            repo: team.repo.clone(),
            login: login.clone(),
        };
        self.link.tell(team.others.clone(), remove);

        if self.link.with(|link| link.rosters.remove_member(&team.repo, login)).is_some() {
            self.link.wake();
        }
    }
}

/// Follows `member`'s board of `repo` until stopped: subscribes through
/// the first of their devices that answers, passes on what it hears, and
/// dials again a while after the link drops.
async fn follow(link: TeamLink, repo: TeamRepo, member: Member, heard: mpsc::Sender<Followed>) {
    loop {
        if let Err(e) = subscribe(&link, &repo, &member, &heard).await {
            log::debug!("the board of {} on {repo} dropped: {e}", member.login);
        }

        let gone = Followed {
            repo: repo.clone(),
            mesh: member.mesh,
            heard: Heard::Gone,
        };
        if heard.send(gone).await.is_err() {
            return;
        }

        sleep(REDIAL).await;
    }
}

/// Subscribes to `member`'s board through each of their devices in turn
/// until one serves it, and passes on what it says until that link drops.
async fn subscribe(
    link: &TeamLink,
    repo: &TeamRepo,
    member: &Member,
    heard: &mpsc::Sender<Followed>,
) -> Result<(), TeamError> {
    let mut failed = TeamError::Unreachable;

    for device in &member.devices {
        match listen(link, repo, member, device, heard).await {
            Ok(()) => return Ok(()),
            Err(e) => failed = e,
        }
    }

    Err(failed)
}

/// Follows `repo`'s board on one of `member`'s devices; an error when the
/// device does not serve it.
async fn listen(
    link: &TeamLink,
    repo: &TeamRepo,
    member: &Member,
    device: &EndpointAddr,
    heard: &mpsc::Sender<Followed>,
) -> Result<(), TeamError> {
    let (mut tx, mut rx) = link.dial(std::slice::from_ref(device)).await?;
    rx.set_max_frame(MAX_FRAME);
    tx.send(&TeamMsg::Subscribe { repo: repo.clone() }).await?;
    let mut served = false;

    loop {
        let msg = match rx.recv::<TeamMsg>().await {
            Ok(Some(msg)) => msg,
            _ if served => return Ok(()),
            Ok(None) => return Err(TeamError::Refused("the board was not served".to_owned())),
            Err(e) => return Err(e.into()),
        };
        let said = match msg {
            TeamMsg::Board { repo: of, cards } if of == *repo => Heard::Board(cards),
            TeamMsg::BoardChange {
                repo: of,
                card,
                gone,
            } if of == *repo => Heard::Change(card, gone),
            TeamMsg::Refused { reason } => return Err(TeamError::Refused(reason)),
            _ => continue,
        };
        served = true;
        let followed = Followed {
            repo: repo.clone(),
            mesh: member.mesh,
            heard: said,
        };

        if heard.send(followed).await.is_err() {
            return Ok(());
        }
    }
}

/// The branch checked out in `folder`; `None` when detached or unknown.
async fn branch_of(folder: &str) -> Option<String> {
    let args = ["-C", folder, "rev-parse", "--abbrev-ref", "HEAD"].map(str::to_owned);
    let out = timeout(GIT_LIMIT, command("git", &args).output())
        .await
        .ok()?
        .ok()?;
    let branch = String::from_utf8(out.stdout).ok()?.trim().to_owned();

    (out.status.success() && !branch.is_empty() && branch != "HEAD").then_some(branch)
}

#[cfg(test)]
#[path = "../tests/team_gateway.rs"]
mod tests;
