//! The gateway: the one machine of this mesh that acts for a team project,
//! elected among the mesh's machines with its checkout as the CI watcher is
//! (`machine::elect`). It serves this mesh's cards to the teammates who
//! subscribe, follows each teammate's board for this mesh's apps on the
//! topic `team.<repo>` (`topic`), and keeps this mesh's place on the team
//! (`membership`).

use super::{
    board::{CHANGES, Followed, Heard, follow},
    cards::{Facts, IssueTitles, Teammates, card_of, settle},
    home::Project,
    link::TeamLink,
    membership::{self, Membership, Team},
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
    team::{Card, Lane, Member, TeamRepo},
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
/// How long a forge call or a branch read may take.
pub(super) const FORGE_LIMIT: Duration = Duration::from_secs(30);
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
        membership: Membership::new(me),
        follows: BTreeMap::new(),
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
    /// This mesh's place on the held teams.
    membership: Membership,
    /// The follow of each teammate's board, by repository and their mesh.
    follows: BTreeMap<(TeamRepo, EndpointId), (Member, AbortOnDropHandle<()>)>,
}

impl Gateway {
    async fn step(&mut self) {
        let (teams, home) = self
            .link
            .with(|link| (membership::teams(link), link.home.clone()));

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
        let held = &self.held;
        self.titles.retain(|repo, _| held.contains(repo));
        self.membership.keep(held);
        self.link.with(|link| {
            link.gateways = gateways;
            link.boards.keep(held);
            link.teammates.retain(|repo, _| held.contains(repo));

            for repo in held {
                if !link.boards.serves(repo) {
                    link.boards.set(repo, Vec::new());
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
        self.serve_cards(&teams, &home.projects).await;
        self.membership.upkeep(&self.link, &teams).await;
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
        let listed = in_the_mesh(
            agents,
            Some(&self.mesh),
            &SessionId::default(),
            Some(&machines),
        )
        .await;
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
            let mut fresh: Vec<Card> = listed
                .iter()
                .filter_map(|listed| {
                    let mut view = listed.view.clone();
                    view.machine = listed.machine.clone();
                    let branch = self.branches.get(&view.id).cloned().flatten();
                    card_of(&view, &facts, branch.as_deref())
                })
                .collect();

            self.link.with(|link| {
                fresh.extend(link.asks.cards(&team.repo, now));
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

        let read = timeout(FORGE_LIMIT, self.link.forge().open_issues(repo))
            .await
            .unwrap_or_else(|_| Err("it took too long".to_owned()))
            .map(|issues| {
                let titles = issues.into_iter().map(|issue| (issue.number, issue.title));
                titles.collect::<IssueTitles>()
            });
        let titles = read.unwrap_or_else(|e| {
            log::info!("the issues of {repo} did not read: {e}");
            self.titles
                .get(repo)
                .map(|(titles, _)| titles.clone())
                .unwrap_or_default()
        });
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
            self.link
                .with(|link| link.teammates.entry(repo.clone()).or_default().gone(mesh));
            self.publish(&repo);
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

    /// Takes what a follow heard, and hands the teammates' cards to this
    /// mesh's apps.
    fn heard(&mut self, followed: Followed) {
        let Followed { repo, mesh, heard } = followed;
        let Some((member, _)) = self.follows.get(&(repo.clone(), mesh)) else {
            return;
        };
        self.link.with(|link| {
            let teammates = link.teammates.entry(repo.clone()).or_default();

            match heard {
                Heard::Board(cards) => teammates.board(mesh, &member.login, cards),
                Heard::Change(card, gone) => teammates.change(mesh, &member.login, card, gone),
                Heard::Gone => teammates.gone(mesh),
            }
        });

        self.publish(&repo);
    }

    fn publish(&self, repo: &TeamRepo) {
        let cards = self.link.with(|link| {
            let teammates = link.teammates.get(repo);
            teammates.map(Teammates::cards).unwrap_or_default()
        });
        let published = rmp_serde::to_vec(&cards)
            .map_err(anyhow::Error::from)
            .and_then(|bytes| self.offers.agents.events.publish(topic(repo), bytes));

        if let Err(e) = published {
            log::warn!("the teammates' cards of {repo} were not published: {e:#}");
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
