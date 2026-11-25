//! The gateway: the one machine of this mesh that acts for a team project,
//! elected among the mesh's machines with its checkout as the CI watcher is
//! (`machine::elect`). It serves this mesh's cards to the teammates who
//! subscribe, follows each teammate's board for this mesh's apps, which read
//! it with `TeamAppArgs::Cards`, and keeps this mesh's place on the team
//! (`membership`).

use super::{
    board::{CHANGES, Followed, Heard, follow},
    cards::{Facts, IssueTitles, card_of, midnight, settle},
    home::{Home, Project},
    link::TeamLink,
    membership::{self, Membership, Team},
    pickup::{Pickups, Round, read_assigned},
    sightings::{self, Sight, Sighted},
};
use crate::{
    agents::{
        ci::Ci,
        mcp::{
            command_name,
            children::{ChildView, Listed, in_the_mesh},
            projects,
        },
    },
    link::Offers,
    machine::{
        ci::forge::command,
        elect::{Reaches, elect, gather, online, ranked},
    },
};
use beep_mesh::Mesh;
use beep_projects::commands::OpenIssue;
use beep_proto::{
    SessionId,
    node::{
        agent::{SessionInfo, Status},
        tools::ForgeReach,
    },
    team::{Card, Lane, Member, TeamRepo},
};
use futures::future::join_all;
use iroh::EndpointId;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    mem::take,
    path::PathBuf,
    time::{Duration, SystemTime},
};
use tokio::{
    select,
    sync::{broadcast, mpsc},
    task::{JoinError, JoinSet},
    time::{Instant, sleep_until, timeout},
};
use tokio_util::task::AbortOnDropHandle;

/// How soon the cards are made again after a session changed, so that a burst
/// of changes makes them once.
const SETTLE: Duration = Duration::from_secs(3);
/// How often the cards are made again while a machine refuses to push its
/// sessions, as one from before it: they are listed from it then.
const ROUND: Duration = Duration::from_secs(10);
/// How often the issue titles, and the issues pickup starts sessions on,
/// are read while a team session works, and otherwise; the machines are
/// asked how they reach the hosts every `QUIET`. The cards are also made
/// again every `BUSY`, as the forge says nothing of its own.
const BUSY: Duration = Duration::from_secs(60);
const QUIET: Duration = Duration::from_secs(10 * 60);
/// How long a forge call or a branch read may take.
pub(super) const FORGE_LIMIT: Duration = Duration::from_secs(30);
const GIT_LIMIT: Duration = Duration::from_secs(5);

/// The machine that acts for `repo`, as `me` sees it with the online
/// `peers`: one of the project's machines on `mine`, this mesh's entry on the
/// team, since teammates turn away any other. When the entry names none of
/// them, as when the last checkout moved before its new machine was
/// announced, any of them, so that it announces itself.
pub(crate) fn elected(
    project: &Project,
    mine: &Member,
    repo: &TeamRepo,
    me: EndpointId,
    ci: &Ci,
    peers: &BTreeMap<EndpointId, ForgeReach>,
) -> Option<EndpointId> {
    let all: BTreeSet<EndpointId> = project.devices.iter().map(|device| device.id).collect();
    let on: BTreeSet<EndpointId> = all
        .iter()
        .copied()
        .filter(|machine| mine.devices.iter().any(|device| device.id == *machine))
        .collect();
    let machines = if on.is_empty() { all } else { on };

    elect(&machines, me, ci.reach_of(&repo.host), peers, &repo.host)
}

/// Acts for the team projects this node is elected for, until the mesh is gone.
/// Pickup keeps what it remembers in `dir`, the node's folder.
pub(crate) async fn run(
    mesh: Mesh,
    me: EndpointId,
    link: TeamLink,
    offers: Offers,
    dir: PathBuf,
    reaches: Reaches,
) {
    let (heard, hearing) = mpsc::channel(CHANGES);

    Gateway::new(mesh, me, link, offers, heard, dir, reaches)
        .drive(hearing)
        .await;
}

struct Gateway {
    mesh: Mesh,
    me: EndpointId,
    link: TeamLink,
    offers: Offers,
    heard: mpsc::Sender<Followed>,
    dir: PathBuf,
    /// What the online machines said of their reach, and when they were
    /// asked and which were asked then.
    peers: BTreeMap<EndpointId, ForgeReach>,
    asked: Option<(Instant, BTreeSet<EndpointId>)>,
    /// The machines being asked now: at most one gather, a newer one
    /// replaces it.
    asking: JoinSet<BTreeMap<EndpointId, ForgeReach>>,
    /// Whether the machines have answered once, so that an election has their
    /// reach to go on.
    answered: bool,
    held: BTreeSet<TeamRepo>,
    /// What each other machine of a held team pushed of its sessions, and the
    /// follows that hear it.
    rows: BTreeMap<EndpointId, Vec<SessionInfo>>,
    sightings: BTreeMap<EndpointId, AbortOnDropHandle<()>>,
    sighted: mpsc::Sender<Sighted>,
    sighting: Option<mpsc::Receiver<Sighted>>,
    reaches: Reaches,
    /// The machines that do not push their sessions.
    refused: BTreeSet<EndpointId>,
    /// A session changed while the cards were made.
    stale: bool,
    /// When the next round of cards is due; none while no team is held or a
    /// round is still going.
    round_due: Option<Instant>,
    /// The queues other machines' gateways handed this one, for pickup to keep.
    handed: BTreeMap<TeamRepo, Vec<u64>>,
    /// What the cards are made from; away in `serving` while they are made.
    cards: Option<Cards>,
    serving: JoinSet<(Cards, Vec<Made>)>,
    /// This mesh's place on the held teams.
    membership: Membership,
    /// The checks of the members on the forge under way.
    checking: JoinSet<()>,
    /// The follow of each teammate's board, by repository and their mesh.
    follows: BTreeMap<(TeamRepo, EndpointId), (Member, AbortOnDropHandle<()>)>,
}

/// What the cards of the held teams are made from, kept between rounds.
struct Cards {
    /// Each held repository's open issue titles, and when they were read.
    titles: BTreeMap<TeamRepo, (IssueTitles, Instant)>,
    /// The branch of each session here in a held team's project.
    branches: HashMap<String, Option<String>>,
    /// The sessions started on the issues assigned to this mesh's person.
    pickups: Pickups,
}

/// What a round of cards works from, as the gateway saw it when it began.
struct Job {
    mesh: Mesh,
    me: EndpointId,
    link: TeamLink,
    offers: Offers,
    teams: Vec<Team>,
    projects: BTreeMap<TeamRepo, Project>,
    peers: BTreeMap<EndpointId, ForgeReach>,
    rows: BTreeMap<EndpointId, Vec<SessionInfo>>,
    refused: BTreeSet<EndpointId>,
}

/// The cards a round made for a team's board.
struct Made {
    repo: TeamRepo,
    cards: Vec<Card>,
    now: SystemTime,
}

impl Gateway {
    fn new(
        mesh: Mesh,
        me: EndpointId,
        link: TeamLink,
        offers: Offers,
        heard: mpsc::Sender<Followed>,
        dir: PathBuf,
        reaches: Reaches,
    ) -> Self {
        let (sighted, sighting) = mpsc::channel(CHANGES);

        Self {
            mesh,
            me,
            link,
            offers,
            heard,
            sighting: Some(sighting),
            peers: BTreeMap::new(),
            asked: None,
            asking: JoinSet::new(),
            answered: false,
            held: BTreeSet::new(),
            rows: BTreeMap::new(),
            sightings: BTreeMap::new(),
            sighted,
            reaches,
            refused: BTreeSet::new(),
            stale: false,
            round_due: None,
            handed: BTreeMap::new(),
            cards: Some(Cards::new(dir.clone())),
            serving: JoinSet::new(),
            membership: Membership::default(),
            checking: JoinSet::new(),
            follows: BTreeMap::new(),
            dir,
        }
    }

    /// Serves until the mesh is gone. Nothing slow runs in this loop: the
    /// machines are asked, the cards made and the members checked in
    /// tasks beside it, and teammates' updates are taken meanwhile. It
    /// elects when the mesh, a roster, the home or a host's reach changes;
    /// a machine that acts for no team has no timer.
    async fn drive(mut self, mut hearing: mpsc::Receiver<Followed>) {
        let mut sighting = self.sighting.take().expect("a gateway is driven once");
        let mut updates = self.mesh.updates();
        let mut home = self.link.home_changes();
        let mut edits = self.link.roster_edits();
        let mut reaches = self.offers.agents.ci.reaches();
        let mut sessions = self.offers.agents.events.subscribe();
        let mut pushed = self.reaches.changes();

        self.step();

        loop {
            let round = if self.held.is_empty() { None } else { self.round_due };
            let ask = ask_again_at(self.asked.as_ref());

            select! {
                // A machine that does not push its reach is asked again when it is due:
                // a machine that acts for no team would not elect once the acting machine stepped down.
                _ = async {
                    match ask {
                        Some(due) => sleep_until(due).await,
                        None => std::future::pending().await,
                    }
                } => self.step(),
                _ = async {
                    match round {
                        Some(due) => sleep_until(due).await,
                        None => std::future::pending().await,
                    }
                } => {
                    self.round_due = None;
                    self.step();
                }
                changed = edits.changed() => match changed {
                    Ok(()) => self.step(),
                    Err(_) => return,
                },
                changed = reaches.changed() => match changed {
                    Ok(()) => self.step(),
                    Err(_) => return,
                },
                Some(followed) = hearing.recv() => self.heard(followed),
                Some(sighted) = sighting.recv() => self.sighted(sighted),
                // A machine pushed how it reaches the hosts: elect on it.
                changed = pushed.changed() => match changed {
                    Ok(()) => {
                        self.asked = None;
                        self.step();
                    }
                    Err(_) => return,
                },
                // This machine's own sessions.
                change = sessions.recv() => match change {
                    Ok(change) if change.topic.name() == "agents" => self.cards_stale(),
                    Ok(_) => {}
                    Err(broadcast::error::RecvError::Lagged(_)) => self.cards_stale(),
                    Err(broadcast::error::RecvError::Closed) => return,
                },
                // The mesh's teams or projects changed: elect on them now.
                changed = home.changed() => match changed {
                    Ok(()) => self.step(),
                    Err(_) => return,
                },
                Some(done) = self.asking.join_next(), if !self.asking.is_empty() => {
                    // A gather that a newer one replaced ends cancelled.
                    if let Ok(peers) = done {
                        self.peers = peers;
                        self.answered = true;
                        let (teams, home) = self.snapshot();
                        self.elect(teams, home);
                    }
                }
                Some(done) = self.serving.join_next(), if !self.serving.is_empty() => {
                    self.served(done);
                }
                Some(_) = self.checking.join_next(), if !self.checking.is_empty() => {}
                changed = updates.changed() => match changed {
                    Ok(()) => self.step(),
                    Err(_) => return,
                },
            }
        }
    }

    /// The teams of the mesh's projects and the home they are in.
    fn snapshot(&self) -> (Vec<Team>, Home) {
        let home = self.link.home(Home::clone);
        let teams = self.link.with(|link| membership::teams(&home, link));

        (teams, home)
    }

    /// Asks the machines when that is due, and elects once they have answered.
    fn step(&mut self) {
        let (teams, home) = self.snapshot();

        self.ask_peers(&teams, &home.projects);

        // The first election waits for the first answers: another machine
        // may be the better one.
        if self.answered {
            self.elect(teams, home);
        }
    }

    /// Elects the gateway of each team, takes the teams this machine acts
    /// for and starts what they need: their announcements, the follow of
    /// their teammates, their cards and the check of their members.
    fn elect(&mut self, teams: Vec<Team>, home: Home) {
        let online = online(&self.mesh);
        let peers: BTreeMap<EndpointId, ForgeReach> = self
            .peers
            .iter()
            .filter(|(peer, _)| online.contains(*peer))
            .map(|(peer, reach)| (*peer, reach.clone()))
            .collect();
        let ci = &self.offers.agents.ci;
        let me = self.me;
        let gateways: BTreeMap<TeamRepo, EndpointId> = teams
            .iter()
            .filter_map(|team| {
                let project = &home.projects[&team.repo];
                let gateway = elected(project, &team.mine, &team.repo, me, ci, &peers)?;
                Some((team.repo.clone(), gateway))
            })
            .collect();
        let teams: Vec<Team> = teams
            .into_iter()
            .filter(|team| gateways.get(&team.repo) == Some(&me))
            .collect();
        self.held = teams.iter().map(|team| team.repo.clone()).collect();
        self.membership.keep(&self.held);
        self.handed.extend(self.link.take_queues());
        self.keep_cards();
        let held = &self.held;
        self.link.publish(|served| {
            served.gateways = gateways;
            served.boards.keep(held);
            served.teammates.retain(|repo, _| held.contains(repo));

            for repo in held {
                if !served.boards.serves(repo) {
                    served.boards.set(repo, Vec::new());
                }
            }
        });

        let mesh = &self.mesh;
        let mut machines = online;
        machines.insert(me);
        let member = |id| mesh.is_member(id);
        self.membership
            .announce(&self.link, &machines, member, &teams, &home.projects);
        self.follow_teammates(&teams);
        self.follow_machines(&teams, &home.projects);
        self.check_members(&teams);
        self.serve_cards(teams, home.projects);
    }

    /// Asks the online machines with a team project how they reach the
    /// hosts, every `QUIET` and when one of them comes or goes, or a team
    /// brings others, all at once, in place of an ask still going. The loop
    /// elects when the answers arrive.
    fn ask_peers(&mut self, teams: &[Team], projects: &BTreeMap<TeamRepo, Project>) {
        let online = online(&self.mesh);
        let asked: BTreeSet<EndpointId> = teams
            .iter()
            .flat_map(|team| &projects[&team.repo].devices)
            .map(|device| device.id)
            .filter(|machine| online.contains(machine))
            .collect();

        if !ask_due(self.asked.as_ref(), &asked) {
            return;
        }

        self.asked = Some((Instant::now(), asked.clone()));
        let reaches = self.reaches.clone();
        self.asking.abort_all();
        self.asking.spawn(async move {
            let asks = asked.into_iter().map(|peer| (peer, reaches.ask(peer)));

            gather("team gateway", asks).await
        });
    }

    /// Hands pickup what is to be kept when it is here to take it.
    fn keep_cards(&mut self) {
        if let Some(cards) = &mut self.cards {
            cards.keep(&self.held, std::mem::take(&mut self.handed));
        }
    }

    /// Starts making the cards of every held team, which teammates and this
    /// mesh's agents read, and starting sessions on new issues where pickup
    /// is due. A round still going makes this tick's wait for the next.
    fn serve_cards(&mut self, teams: Vec<Team>, projects: BTreeMap<TeamRepo, Project>) {
        if self.round_due.is_some_and(|due| due > Instant::now()) {
            return;
        }

        let Some(mut cards) = self.cards.take() else {
            self.stale = true;
            return;
        };
        self.round_due = None;
        let job = Job {
            mesh: self.mesh.clone(),
            me: self.me,
            link: self.link.clone(),
            offers: self.offers.clone(),
            teams,
            projects,
            peers: self.peers.clone(),
            rows: self.rows.clone(),
            refused: self.refused.clone(),
        };
        self.serving.spawn(async move {
            let made = cards.make(&job).await;

            (cards, made)
        });
    }

    /// Takes the cards a round made, for this mesh's boards and the teammates.
    fn served(&mut self, done: Result<(Cards, Vec<Made>), JoinError>) {
        let (cards, made) = match done {
            Ok(done) => done,
            Err(e) => {
                // What pickup remembers is on disk; the rest is read again.
                log::warn!("the team cards were not made: {e}");
                (Cards::new(self.dir.clone()), Vec::new())
            }
        };
        self.cards = Some(cards);
        self.keep_cards();
        let wait = match (take(&mut self.stale), self.refused.is_empty()) {
            (true, _) => SETTLE,
            (false, true) => BUSY,
            (false, false) => ROUND,
        };
        self.round_due = Some(Instant::now() + wait);
        let held = &self.held;

        self.link.publish(|served| {
            for Made { repo, cards, now } in made.into_iter().filter(|made| held.contains(&made.repo)) {
                let old = served.boards.cards(&repo).unwrap_or_default();
                served.boards.set(&repo, settle(&old, cards, now));
            }
        });
    }

    /// Starts the check of the members of each held team on the forge that
    /// is due, all together.
    fn check_members(&mut self, teams: &[Team]) {
        let due = self.membership.due(teams);

        if due.is_empty() {
            return;
        }

        let link = self.link.clone();
        self.checking.spawn(async move {
            join_all(due.iter().map(|team| membership::check_members(&link, team))).await;
        });
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
            self.link
                .publish(|served| served.teammates.entry(repo.clone()).or_default().gone(mesh));
        }

        for (key, member) in wanted {
            if self.follows.contains_key(&key) {
                continue;
            }

            let task = follow(
                self.link.clone(),
                key.0.clone(),
                member.clone(),
                self.heard.clone(),
            );
            self.follows
                .insert(key, (member, AbortOnDropHandle::new(tokio::spawn(task))));
        }
    }

    /// Starts making the cards again soon: a session of this mesh changed.
    /// One that changes while the cards are made makes them again after.
    fn cards_stale(&mut self) {
        if self.held.is_empty() {
            return;
        }

        if self.cards.is_none() {
            self.stale = true;
            return;
        }

        let due = Instant::now() + SETTLE;
        self.round_due = Some(self.round_due.map_or(due, |at| at.min(due)));
    }

    /// Takes what a follow heard of another machine's sessions.
    fn sighted(&mut self, sighted: Sighted) {
        let Sighted { peer, sight } = sighted;

        if !self.sightings.contains_key(&peer) {
            return;
        }

        match sight {
            Sight::Rows(rows) => {
                self.rows.insert(peer, rows);
            }
            Sight::Gone => {
                self.rows.remove(&peer);
            }
            Sight::Refused => {
                self.rows.remove(&peer);
                self.refused.insert(peer);
            }
        }

        self.cards_stale();
    }

    /// Follows the sessions of the other online machines of the held teams,
    /// and stops following those that went offline or left them.
    fn follow_machines(&mut self, teams: &[Team], projects: &BTreeMap<TeamRepo, Project>) {
        let online = online(&self.mesh);
        let wanted: BTreeSet<EndpointId> = teams
            .iter()
            .flat_map(|team| &projects[&team.repo].devices)
            .map(|device| device.id)
            .filter(|machine| *machine != self.me && online.contains(machine))
            .collect();

        self.sightings.retain(|peer, _| wanted.contains(peer));
        self.rows.retain(|peer, _| wanted.contains(peer));
        self.refused.retain(|peer| wanted.contains(peer));

        for peer in wanted {
            self.sightings.entry(peer).or_insert_with(|| {
                let task = sightings::follow(self.mesh.clone(), peer, self.sighted.clone());

                AbortOnDropHandle::new(tokio::spawn(task))
            });
        }
    }

    /// Takes what a follow heard, for this mesh's apps.
    fn heard(&mut self, followed: Followed) {
        let Followed { repo, mesh, heard } = followed;
        let Some((member, _)) = self.follows.get(&(repo.clone(), mesh)) else {
            return;
        };
        self.link.publish(|served| {
            let teammates = served.teammates.entry(repo.clone()).or_default();

            match heard {
                Heard::Board(cards) => teammates.board(mesh, &member.login, cards),
                Heard::Change(card, gone) => teammates.change(mesh, &member.login, card, gone),
                Heard::Gone => teammates.gone(mesh),
            }
        });
    }
}

impl Cards {
    fn new(dir: PathBuf) -> Self {
        Self {
            titles: BTreeMap::new(),
            branches: HashMap::new(),
            pickups: Pickups::new(dir),
        }
    }

    /// Forgets the teams that are not `held` here, and has pickup keep the
    /// queues `handed` by the gateways of the others.
    fn keep(&mut self, held: &BTreeSet<TeamRepo>, handed: BTreeMap<TeamRepo, Vec<u64>>) {
        self.titles.retain(|repo, _| held.contains(repo));
        self.pickups.keep(held, handed);
    }

    /// Makes the cards of every team of `job`, and starts sessions on new
    /// issues where pickup is due. What the forge and the machines say is
    /// read together.
    async fn make(&mut self, job: &Job) -> Vec<Made> {
        let Job {
            mesh,
            link,
            offers,
            teams,
            projects,
            rows,
            refused,
            ..
        } = job;
        let mut picking = BTreeMap::new();
        let mut on = BTreeSet::new();

        for team in teams {
            let record = link.with(|link| link.rosters.written(&team.repo).cloned());

            match record.filter(|record| record.pickup) {
                Some(record) => {
                    on.insert(&team.repo);

                    if self.titles_due(link, &team.repo) {
                        picking.insert(&team.repo, record.pickup_limit);
                    }
                }
                None => self.pickups.off(&team.repo),
            }
        }

        if teams.is_empty() {
            return Vec::new();
        }

        let due: Vec<&Team> = teams
            .iter()
            .filter(|team| self.titles_due(link, &team.repo))
            .collect();
        let caller = SessionId::default();
        let ended_since = midnight(SystemTime::now());
        // The machines that push their sessions are heard; the others are asked.
        let sessions = async {
            let mut listed =
                in_the_mesh(&offers.agents, Some(mesh), &caller, Some(refused), Some(ended_since)).await;
            listed.extend(
                rows.iter()
                    .flat_map(|(peer, rows)| pushed_sessions(mesh, *peer, rows, ended_since)),
            );

            listed
        };
        let titles = join_all(due.iter().map(|team| read_titles(link, &team.repo)));
        let assigned = join_all(
            teams
                .iter()
                .filter(|team| picking.contains_key(&team.repo))
                .map(|team| async move { (&team.repo, read_assigned(link, team).await) }),
        );
        let (listed, titles, assigned) = tokio::join!(sessions, titles, assigned);
        let mut assigned: BTreeMap<&TeamRepo, Option<Vec<OpenIssue>>> = assigned.into_iter().collect();
        let read: BTreeMap<&TeamRepo, IssueTitles> = due
            .iter()
            .zip(titles)
            .map(|(team, titles)| (&team.repo, self.keep_titles(&team.repo, titles)))
            .collect();
        self.branches
            .retain(|id, _| listed.iter().any(|listed| listed.view.id == *id));
        let mut made = Vec::new();

        for team in teams {
            let (titles, read) = match read.get(&team.repo) {
                Some(titles) => (titles.clone(), true),
                None => (self.cached(&team.repo), false),
            };
            self.read_branches(&listed, &team.project, read).await;
            let now = SystemTime::now();
            let facts = Facts {
                project: &team.project,
                login: &team.mine.login,
                linked: &titles,
                now,
            };
            let mut fresh: Vec<Card> = listed
                .iter()
                .filter_map(|listed| {
                    let mut view = listed.view.clone();
                    view.machine = listed.machine.clone();
                    // Read here only of this machine's sessions; another
                    // machine lists its sessions' branches.
                    let read = self.branches.get(&view.id).cloned().flatten();
                    let branch = read.or_else(|| view.branch.clone());
                    card_of(&view, &facts, branch.as_deref())
                })
                .collect();

            if let Some(limit) = picking.get(&team.repo)
                && let Some(Some(assigned)) = assigned.remove(&team.repo)
            {
                let project = &projects[&team.repo];
                let round = Round {
                    link,
                    mesh,
                    agents: &offers.agents,
                    team,
                    project,
                    cards: &fresh,
                    elsewhere: elsewhere(job, project, &team.repo),
                };
                self.pickups.round(round, assigned, *limit).await;
            }

            let others = starters(&projects[&team.repo], &online(mesh));
            let on = on.contains(&team.repo);
            self.pickups.hand(mesh, &team.repo, on, others);
            fresh.extend(self.pickups.queued(&team.repo));
            made.push(Made {
                repo: team.repo.clone(),
                cards: fresh,
                now,
            });
        }

        made
    }

    /// Reads the branch of each session here in `project` not read yet, or
    /// of all of them `again`.
    async fn read_branches(&mut self, listed: &[Listed], project: &str, again: bool) {
        let unread: Vec<(String, String)> = listed
            .iter()
            .filter(|listed| listed.view.project.as_deref() == Some(project))
            .filter(|listed| again || !self.branches.contains_key(&listed.view.id))
            .filter_map(|listed| Some((listed.view.id.clone(), listed.folder.clone()?)))
            .collect();
        let read = join_all(unread.iter().map(|(_, folder)| branch_of(folder))).await;

        self.branches
            .extend(unread.into_iter().map(|(id, _)| id).zip(read));
    }

    /// Whether `repo`'s issues are read again: every `BUSY` while a session
    /// on its board works, every `QUIET` otherwise.
    fn titles_due(&self, link: &TeamLink, repo: &TeamRepo) -> bool {
        let cards = link.served(|served| served.boards.cards(repo)).unwrap_or_default();
        let working = cards.iter().any(|card| card.lane == Lane::Working {});
        let wait = if working { BUSY } else { QUIET };

        self.titles
            .get(repo)
            .is_none_or(|(_, at)| at.elapsed() >= wait)
    }

    /// `repo`'s open issue titles as last read.
    fn cached(&self, repo: &TeamRepo) -> IssueTitles {
        self.titles
            .get(repo)
            .map(|(titles, _)| titles.clone())
            .unwrap_or_default()
    }

    /// Keeps what was just `read` of `repo`'s issues, or what was read
    /// before when it did not read; the titles to make cards from.
    fn keep_titles(&mut self, repo: &TeamRepo, read: Result<IssueTitles, String>) -> IssueTitles {
        let titles = read.unwrap_or_else(|e| {
            log::info!("the issues of {repo} did not read: {e}");
            self.cached(repo)
        });
        self.titles
            .insert(repo.clone(), (titles.clone(), Instant::now()));

        titles
    }
}

/// `repo`'s open issue titles, from the forge.
async fn read_titles(link: &TeamLink, repo: &TeamRepo) -> Result<IssueTitles, String> {
    let read = timeout(FORGE_LIMIT, link.forge().open_issues(repo))
        .await
        .unwrap_or_else(|_| Err("it took too long".to_owned()))?;
    let titles = read.into_iter().map(|issue| (issue.number, issue.title));

    Ok(titles.collect())
}

/// The machines of `project` that start its issues' sessions when this
/// one runs no agent or refuses: the other online ones with its
/// checkout, desktops and nodes, ranked as the gateway is elected.
fn elsewhere(job: &Job, project: &Project, repo: &TeamRepo) -> Vec<EndpointId> {
    let reach = job.offers.agents.ci.reach_of(&repo.host);
    let machines = starters(project, &online(&job.mesh));

    ranked(&machines, job.me, reach, &job.peers, &repo.host)
}

/// Whether the machines are asked of their reach again: every `QUIET`, and
/// whenever those to ask are not the ones `asked` last.
fn ask_due(asked: Option<&(Instant, BTreeSet<EndpointId>)>, now: &BTreeSet<EndpointId>) -> bool {
    asked.is_none_or(|(at, then)| at.elapsed() >= QUIET || then != now)
}

/// When the machines are asked of their reach again, while there are any to ask.
fn ask_again_at(asked: Option<&(Instant, BTreeSet<EndpointId>)>) -> Option<Instant> {
    let (at, machines) = asked?;

    (!machines.is_empty()).then(|| *at + QUIET)
}

/// `rows`, the sessions `peer` pushed, as the agents list names them: those
/// that run, and those that ended since `ended_since`, Unix seconds.
fn pushed_sessions(
    mesh: &Mesh,
    peer: EndpointId,
    rows: &[SessionInfo],
    ended_since: u64,
) -> Vec<Listed> {
    let Some(status) = mesh
        .code_hosts()
        .into_iter()
        .find(|status| status.device.id == peer)
    else {
        return Vec::new();
    };
    let name = command_name(&status.device.name);

    rows.iter()
        .filter(|info| !info.reviewer)
        .filter(|info| info.state.status != Status::Ended || info.last_activity >= ended_since)
        .map(|info| {
            let mut view = ChildView::of(info, &info.id).named(&name);
            view.project = projects::of_folder(mesh, &info.cwd, peer);

            Listed {
                view,
                machine: status.device.name.clone(),
                folder: None,
            }
        })
        .collect()
}

/// The machines of `project` that may start its issues' sessions for the
/// gateway: those with its checkout among the `online` ones, desktops too.
pub(super) fn starters(project: &Project, online: &BTreeSet<EndpointId>) -> BTreeSet<EndpointId> {
    project
        .devices
        .iter()
        .map(|device| device.id)
        .filter(|machine| online.contains(machine))
        .collect()
}

/// The branch checked out in `folder`; `None` when detached or unknown.
pub(crate) async fn branch_of(folder: &str) -> Option<String> {
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
