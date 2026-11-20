//! The gateway: the one machine of this mesh that acts for a team project,
//! elected among the mesh's machines with its checkout as the CI watcher is
//! (`machine::elect`). It serves this mesh's cards to the teammates who
//! subscribe, follows each teammate's board for this mesh's apps on the
//! topic `team.<repo>` (`topic`), and checks once a day that every member
//! may still push.

use super::{
    board::{CHANGES, Followed, Heard, follow},
    cards::{Facts, IssueTitles, Teammates, card_of, settle},
    home::Project,
    link::TeamLink,
    roster::with_devices,
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
    node::{events::Topic, tools::ForgeReach},
    team::{Card, ForgeLogin, Lane, Member, TeamMsg, TeamRepo},
};
use futures::future::join_all;
use iroh::EndpointId;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    time::{Duration, SystemTime},
};
use tokio::{
    select,
    sync::mpsc,
    time::{Instant, MissedTickBehavior, interval, timeout},
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
/// The least time between two announcements of this mesh's devices on a team.
const ANNOUNCE: Duration = Duration::from_secs(60);
/// How long a forge call or a branch read may take.
const FORGE_LIMIT: Duration = Duration::from_secs(30);
const GIT_LIMIT: Duration = Duration::from_secs(5);

/// The topic this mesh's apps read `repo`'s teammates' cards on.
pub(crate) fn topic(repo: &TeamRepo) -> Topic {
    Topic::keyed("team", repo.to_string())
}

/// The machine that acts for `repo`, as `me` sees it with the online
/// `peers`: one of the project's machines on `mine`, this mesh's entry on the
/// team, since teammates turn away any other.
pub(crate) fn elected(
    project: &Project,
    mine: &Member,
    repo: &TeamRepo,
    me: EndpointId,
    ci: &Ci,
    peers: &BTreeMap<EndpointId, ForgeReach>,
) -> Option<EndpointId> {
    let machines = project
        .devices
        .iter()
        .map(|device| device.id)
        .filter(|machine| mine.devices.iter().any(|device| device.id == *machine))
        .collect();

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
        announced: BTreeMap::new(),
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

/// A team this machine acts for.
struct Team {
    repo: TeamRepo,
    /// The mesh's name for the project.
    project: String,
    /// This mesh's entry on it.
    mine: Member,
    /// The members of other meshes.
    others: Vec<Member>,
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
    /// The branch of each session here in a held team's project.
    branches: HashMap<String, Option<String>>,
    /// The devices last announced on each held team, and when.
    announced: BTreeMap<TeamRepo, (BTreeSet<EndpointId>, Instant)>,
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
                        mine: mine.into_iter().next()?,
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
                elected(project, &team.mine, &team.repo, me, ci, &peers) == Some(me)
            })
            .collect();
        self.held = teams.iter().map(|team| team.repo.clone()).collect();
        let held = &self.held;
        self.titles.retain(|repo, _| held.contains(repo));
        self.checked.retain(|repo, _| held.contains(repo));
        self.announced.retain(|repo, _| held.contains(repo));
        self.link.with(|link| {
            link.boards.keep(held);

            for repo in held {
                if !link.boards.serves(repo) {
                    link.boards.set(repo, Vec::new());
                }
            }
        });

        self.announce(&teams, &home.projects);
        self.follow_teammates(&teams);
        self.serve_cards(&teams, &home.projects).await;
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

    /// Names this mesh's machines with each held project on its entry, when
    /// they changed: once per change, at most every `ANNOUNCE`.
    fn announce(&mut self, teams: &[Team], projects: &BTreeMap<TeamRepo, Project>) {
        let mut online = online(&self.mesh);
        online.insert(self.me);

        for team in teams {
            let devices = &projects[&team.repo].devices;
            let mesh = &self.mesh;
            let Some(entry) = with_devices(&team.mine, devices, &online, |id| mesh.is_member(id))
            else {
                continue;
            };
            let named: BTreeSet<EndpointId> = entry.devices.iter().map(|device| device.id).collect();
            let recent = self
                .announced
                .get(&team.repo)
                .is_some_and(|(sent, at)| *sent == named || at.elapsed() < ANNOUNCE);

            if !recent {
                self.announced
                    .insert(team.repo.clone(), (named, Instant::now()));
                announce(&self.link, self.me, team, entry);
            }
        }
    }

    /// Makes the cards of every held team with a subscriber.
    async fn serve_cards(&mut self, teams: &[Team], projects: &BTreeMap<TeamRepo, Project>) {
        let watched: Vec<&Team> = teams
            .iter()
            .filter(|team| self.link.with(|link| link.boards.watched(&team.repo)))
            .collect();

        if watched.is_empty() {
            return;
        }

        let machines: BTreeSet<EndpointId> = watched
            .iter()
            .flat_map(|team| &projects[&team.repo].devices)
            .map(|device| device.id)
            .collect();
        let agents = &self.offers.agents;
        let listed = in_the_mesh(agents, Some(&self.mesh), &SessionId::default(), Some(&machines)).await;
        self.branches
            .retain(|id, _| listed.iter().any(|listed| listed.view.id == *id));

        for team in watched {
            let (titles, read) = self.read_titles(&team.repo).await;
            self.read_branches(&listed, &team.project, read).await;
            let now = SystemTime::now();
            let facts = Facts {
                project: &team.project,
                login: &team.mine.login,
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

    /// `repo`'s open issue titles, and whether they were read now: again
    /// every `BUSY` while a session on its board works, every `QUIET` otherwise.
    async fn read_titles(&mut self, repo: &TeamRepo) -> (IssueTitles, bool) {
        let working = self.link.with(|link| {
            let cards = link.boards.cards(repo).unwrap_or_default();
            cards.iter().any(|card| card.lane == Lane::Working {})
        });
        let wait = if working { BUSY } else { QUIET };

        if let Some((titles, at)) = self.titles.get(repo)
            && at.elapsed() < wait
        {
            return (titles.clone(), false);
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

        (titles, true)
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
            check_members(&self.link, team).await;
        }
    }
}

/// Takes `entry`, this mesh's entry on `team` with its devices now, as a
/// teammate's roster is taken (for a desktop to write), then sends it to every
/// teammate.
fn announce(link: &TeamLink, me: EndpointId, team: &Team, entry: Member) {
    let roster = TeamMsg::Roster {
        repo: team.repo.clone(),
        members: vec![entry],
    };
    let own = Some(team.mine.mesh);

    match link.with(|link| link.rosters.apply(me, own, &roster)) {
        Ok(applied) => {
            if applied.changed.is_some() {
                link.wake();
            }
            link.tell(team.others.clone(), roster);
        }
        Err(refusal) => log::info!("this mesh's devices on {} did not change: {refusal}", team.repo),
    }
}

/// Asks the forge whether each member of `team` from another mesh may still
/// push, and removes those who may not.
async fn check_members(link: &TeamLink, team: &Team) {
    for member in &team.others {
        let login = &member.login;
        let asked = link.forge().collaborator(&team.repo, login);

        match timeout(FORGE_LIMIT, asked).await {
            Ok(Ok(true)) => {}
            Ok(Ok(false)) => remove(link, team, login),
            Ok(Err(e)) => log::info!("could not check {login} on {}: {e}", team.repo),
            Err(_) => log::info!("the forge took too long to check {login} on {}", team.repo),
        }
    }
}

/// Removes `login` from `team`, as a member's `Remove` does, and tells every
/// other member.
fn remove(link: &TeamLink, team: &Team, login: &ForgeLogin) {
    log::info!("{login} may no longer push to {}: removing them from the team", team.repo);
    let remove = TeamMsg::Remove {
        repo: team.repo.clone(),
        login: login.clone(),
    };
    link.tell(team.others.clone(), remove);

    if link.with(|link| link.rosters.remove_member(&team.repo, login)).is_some() {
        link.wake();
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
