//! Issue pickup: the gateway of a team project starts an agent on each open
//! issue newly assigned to this mesh's person, once ever per issue. Issues
//! already assigned when pickup is turned on are left alone, an optional
//! limit holds the rest as Queued cards, and an automatic session that ends
//! without a pull request becomes a Needs you card for this mesh's apps.

use super::{
    asks::MAX_CARDS, gateway::FORGE_LIMIT, home::Project, link::TeamLink, membership::Team,
};
use crate::{
    agents::{
        Agents,
        mcp::{PickupStart, over_the_mesh, start_issue, still_runs},
    },
    link::note,
};
use beep_mesh::Mesh;
use beep_projects::commands::OpenIssue;
use beep_proto::{
    SessionId,
    node::tools::{Approved, Reply, Request, Tool},
    team::{Card, Lane, TeamRepo},
};
use futures::future::join_all;
use iroh::EndpointId;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    time::{Duration, SystemTime},
};
use tokio::time::timeout;

/// What starts the id of the Needs you card of an automatic session that
/// ended without a pull request, and of a Queued card, before the issue.
const PICKUP_CARD: &str = "pickup:";
const QUEUED_CARD: &str = "queued:";
/// The longest slug of a title an issue's branch takes, as Work on it cuts it.
const BRANCH_SLUG: usize = 30;
/// How long another machine gets to start a session.
const START_LIMIT: Duration = Duration::from_secs(120);

/// What pickup remembers of a team, kept on disk.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Pickup {
    /// The issues an automatic session was started on: never again.
    pub(crate) started: BTreeSet<u64>,
    /// The issues assigned when pickup was turned on: never started.
    pub(crate) seen_at_enable: BTreeSet<u64>,
    /// Whether pickup was on when last seen, to tell when it is turned on.
    pub(crate) on: bool,
    /// The automatic sessions followed until they end, by issue.
    pub(crate) running: BTreeMap<u64, Auto>,
}

/// An automatic session: its card and the branch it works on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Auto {
    pub(crate) card: String,
    pub(crate) branch: String,
}

/// The issues to start now, and those held back by the limit.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Plan {
    pub(crate) start: Vec<u64>,
    pub(crate) queued: Vec<u64>,
}

/// What to do with the `assigned` issues: each not started before, not
/// assigned when pickup was turned on and with no `live` session starts,
/// while fewer than `limit` automatic sessions are live; the rest wait.
pub(crate) fn due(p: &Pickup, assigned: &[u64], live: &BTreeSet<u64>, limit: Option<u32>) -> Plan {
    let mut new: Vec<u64> = assigned
        .iter()
        .copied()
        .filter(|n| !p.started.contains(n) && !p.seen_at_enable.contains(n) && !live.contains(n))
        .collect();
    new.dedup();
    let running = p.started.intersection(live).count();
    let room = limit.map_or(usize::MAX, |limit| {
        usize::try_from(limit)
            .unwrap_or(usize::MAX)
            .saturating_sub(running)
    });
    let queued = new.split_off(room.min(new.len()));

    Plan { start: new, queued }
}

/// The branch Work on it makes for an issue: `issue-52-app-crashes-when`,
/// the title's words cut at a word to fit `BRANCH_SLUG`.
pub(crate) fn issue_branch(number: u64, title: &str) -> String {
    let lower = title.to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_ascii_lowercase() && !c.is_ascii_digit())
        .filter(|word| !word.is_empty())
        .collect();
    let mut slug = words.join("-");

    if slug.len() > BRANCH_SLUG {
        let cut = &slug[..BRANCH_SLUG];
        slug = match cut.rfind('-') {
            Some(at) if at > 0 => cut[..at].to_owned(),
            _ => cut.to_owned(),
        };
    }

    match slug.is_empty() {
        true => format!("issue-{number}"),
        false => format!("issue-{number}-{slug}"),
    }
}

/// The prompt Work on it gives for an issue.
pub(crate) fn issue_prompt(number: u64, title: &str, url: &str) -> String {
    format!(
        "Work on issue #{number}: {title} ({url}). Read the issue and its comments first, then propose and implement a fix."
    )
}

/// The issues of `cards` with a session that has not ended: those its
/// branch names, and those of the automatic sessions in `p`.
fn live(p: &Pickup, cards: &[Card]) -> BTreeSet<u64> {
    let autos = |card: &Card| {
        let running = p.running.iter();
        running
            .filter(|(_, auto)| auto.card == card.id)
            .map(|(n, _)| *n)
            .collect::<Vec<_>>()
    };

    cards
        .iter()
        .filter(|card| card.lane != Lane::DoneToday {})
        .flat_map(|card| card.issue.into_iter().chain(autos(card)))
        .collect()
}

/// The pickup of every team the gateway holds.
pub(super) struct Pickups {
    dir: PathBuf,
    states: BTreeMap<TeamRepo, Pickup>,
    /// Each team's Queued cards, for its board.
    queued: BTreeMap<TeamRepo, Vec<Card>>,
}

/// What one pickup round of a team works from.
pub(super) struct Round<'a> {
    pub(super) link: &'a TeamLink,
    pub(super) mesh: &'a Mesh,
    pub(super) agents: &'a Agents,
    pub(super) team: &'a Team,
    pub(super) project: &'a Project,
    /// This mesh's cards of the team now.
    pub(super) cards: &'a [Card],
    /// The machine that starts sessions when this one runs no agent.
    pub(super) elsewhere: Option<EndpointId>,
}

/// Why a session did not start: refused, or not known, in which case it
/// may run and the issue is not tried again.
enum Unstarted {
    Refused(String),
    Unknown(String),
}

impl Pickups {
    /// Pickup that keeps what it remembers in `<node dir>/team-pickup`.
    pub(super) fn new(dir: PathBuf) -> Self {
        Self {
            dir: dir.join("team-pickup"),
            states: BTreeMap::new(),
            queued: BTreeMap::new(),
        }
    }

    /// The Queued cards of `repo`'s board.
    pub(super) fn queued(&self, repo: &TeamRepo) -> Vec<Card> {
        self.queued.get(repo).cloned().unwrap_or_default()
    }

    /// Notes that pickup is off for `repo`, so turning it on again leaves
    /// the issues assigned by then alone.
    pub(super) fn off(&mut self, repo: &TeamRepo) {
        self.queued.remove(repo);
        let mut state = self.take(repo);

        if state.on {
            state.on = false;
            self.save(repo, &state);
        }

        self.states.insert(repo.clone(), state);
    }

    /// Starts sessions on the issues newly assigned to this mesh's person,
    /// and turns the automatic sessions that ended without a pull request
    /// into Needs you cards.
    pub(super) async fn round(&mut self, round: Round<'_>, limit: Option<u32>) {
        let repo = &round.team.repo;
        let login = &round.team.mine.login;
        let read = timeout(FORGE_LIMIT, round.link.forge().assigned_issues(repo, login)).await;
        let assigned = match read {
            Ok(Ok(assigned)) => assigned,
            Ok(Err(e)) => return log::info!("the issues assigned on {repo} did not read: {e}"),
            Err(_) => return log::info!("the issues assigned on {repo} took too long to read"),
        };
        let mut state = self.take(repo);
        let before = state.clone();

        if !state.on {
            state.on = true;
            state.seen_at_enable = assigned.iter().map(|issue| issue.number).collect();
        }

        ended(&round, &mut state).await;
        let numbers: Vec<u64> = assigned.iter().map(|issue| issue.number).collect();
        let plan = due(&state, &numbers, &live(&state, round.cards), limit);
        let issue = |n: &u64| assigned.iter().find(|issue| issue.number == *n);
        let starts = plan
            .start
            .iter()
            .filter_map(issue)
            .map(|issue| start(&round, issue));

        for (issue, started) in plan.start.iter().zip(join_all(starts).await) {
            match started {
                Ok(auto) => {
                    state.started.insert(*issue);
                    state.running.insert(*issue, auto);
                }
                Err(Unstarted::Unknown(e)) => {
                    log::warn!("a session on {repo}#{issue} may have started: {e}");
                    state.started.insert(*issue);
                }
                Err(Unstarted::Refused(e)) => {
                    log::info!("no session started on {repo}#{issue}: {e}");
                }
            }
        }

        let now = SystemTime::now();
        let queued = plan.queued.iter().filter_map(issue).map(|issue| Card {
            id: format!("{QUEUED_CARD}{}", issue.number),
            title: issue.title.clone(),
            lane: Lane::Queued {},
            login: login.clone(),
            machine: String::new(),
            issue: Some(issue.number),
            pr: None,
            updated_at: now,
        });
        self.queued.insert(repo.clone(), queued.collect());

        if state != before {
            self.save(repo, &state);
        }

        self.states.insert(repo.clone(), state);
    }

    /// `repo`'s state, out of memory or read from disk.
    fn take(&mut self, repo: &TeamRepo) -> Pickup {
        self.states
            .remove(repo)
            .unwrap_or_else(|| note::read(&self.file(repo)))
    }

    fn save(&self, repo: &TeamRepo, state: &Pickup) {
        let saved = std::fs::create_dir_all(&self.dir)
            .map_err(anyhow::Error::from)
            .and_then(|()| note::write_json(&self.file(repo), state));

        if let Err(e) = saved {
            log::warn!("the pickup of {repo} was not saved: {e:#}");
        }
    }

    fn file(&self, repo: &TeamRepo) -> PathBuf {
        let hash = blake3::hash(repo.to_string().as_bytes()).to_hex();
        self.dir.join(format!("{hash}.json"))
    }
}

/// Stops following the automatic sessions that ended, and makes a Needs
/// you card of each without a pull request from its branch. A session has
/// ended once it is off this mesh's cards and its machine no longer lists
/// it: a machine that does not answer, or pull requests that do not read,
/// leave it for the next round.
async fn ended(round: &Round<'_>, state: &mut Pickup) {
    let absent: Vec<(u64, String)> = state
        .running
        .iter()
        .filter(|(_, auto)| !round.cards.iter().any(|card| card.id == auto.card))
        .map(|(n, auto)| (*n, auto.card.clone()))
        .collect();
    let runs = absent.iter().map(|(_, card)| still_runs(round.mesh, card));
    let ended: Vec<u64> = absent
        .iter()
        .zip(join_all(runs).await)
        .filter(|(_, runs)| *runs == Some(false))
        .map(|((n, _), _)| *n)
        .collect();

    if ended.is_empty() {
        return;
    }

    let repo = &round.team.repo;
    let heads = match timeout(FORGE_LIMIT, round.link.forge().pull_heads(repo)).await {
        Ok(Ok(heads)) => heads,
        Ok(Err(e)) => return log::info!("the pull requests of {repo} did not read: {e}"),
        Err(_) => return log::info!("the pull requests of {repo} took too long to read"),
    };

    for n in ended {
        let Some(auto) = state.running.remove(&n) else {
            continue;
        };

        if heads.contains(&auto.branch) {
            continue;
        }

        let id = format!("{PICKUP_CARD}{n}");
        round.link.with(|teams| {
            let cards = teams.asks.cards.entry(repo.clone()).or_default();

            if cards.len() < MAX_CARDS && !cards.iter().any(|card| card.id == id) {
                cards.push(Card {
                    id,
                    title: format!("#{n} ended without a pull request"),
                    lane: Lane::NeedsYou {},
                    login: round.team.mine.login.clone(),
                    machine: String::new(),
                    issue: Some(n),
                    pr: None,
                    updated_at: SystemTime::now(),
                });
            }
        });
    }
}

/// Starts a session on `issue` here, when this machine runs an agent, or
/// on the machine the round names.
async fn start(round: &Round<'_>, issue: &OpenIssue) -> Result<Auto, Unstarted> {
    let order = PickupStart {
        repo: round.team.repo.clone(),
        number: issue.number,
        title: issue.title.clone(),
        url: issue.url.clone(),
    };

    if round.project.checkout.is_some() && round.agents.auto_harness().is_some() {
        return start_issue(round.agents, round.mesh, &order)
            .await
            .map_err(Unstarted::Refused);
    }

    let Some(machine) = round.elsewhere else {
        let none = "no machine of the mesh with the project runs an agent";
        return Err(Unstarted::Refused(none.to_owned()));
    };
    let args = serde_json::to_string(&order).map_err(|e| Unstarted::Refused(e.to_string()))?;
    let request = Request {
        session: SessionId::default(),
        tool: Tool::TeamPickup,
        args,
        approved: Approved::No,
    };

    match timeout(START_LIMIT, over_the_mesh(round.mesh, machine, &request)).await {
        Ok(Ok(Reply::Done {
            text,
            failed: false,
        })) => serde_json::from_str(&text).map_err(|e| Unstarted::Unknown(e.to_string())),
        Ok(Ok(Reply::Done { text, .. })) => Err(Unstarted::Refused(text)),
        Ok(Ok(other)) => Err(Unstarted::Unknown(format!("it answered {other:?}"))),
        Ok(Err(e)) => Err(Unstarted::Refused(format!("{e:#}"))),
        Err(_) => Err(Unstarted::Unknown("it did not answer in time".to_owned())),
    }
}

#[cfg(test)]
#[path = "../tests/team_pickup.rs"]
mod tests;
