//! Issue pickup: the gateway of a team project starts an agent on each open
//! issue newly assigned to this mesh's person, once ever per issue. Issues
//! already assigned when pickup is turned on are left alone, an optional
//! limit holds the rest as Queued cards, and an automatic session that ends
//! without a pull request becomes a Needs you card for this mesh's apps.

use super::{
    asks::MAX_CARDS, cards::TITLE, gateway::FORGE_LIMIT, home::Project, link::TeamLink,
    membership::Team,
};
use crate::{
    agents::{
        Agents,
        mcp::{CallFailed, PickupStart, over_the_mesh, start_issue, still_runs},
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
/// How many refused starts pickup gives an issue before it gives it up.
const ATTEMPTS: u32 = 3;
/// How long a session gets to start and be named by its agent.
const START_LIMIT: Duration = Duration::from_secs(120);

/// What pickup remembers of a team, kept on disk.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Pickup {
    /// The issues an automatic session was started on: never again.
    pub(crate) started: BTreeSet<u64>,
    /// The issues assigned when pickup was turned on: never started.
    pub(crate) seen_at_enable: BTreeSet<u64>,
    /// Whether pickup was on in this process's last round of the team. A
    /// team read from disk starts off, so the first round after the node
    /// starts, as after pickup is turned on or the team comes back to this
    /// machine, takes the issues assigned by then as the backlog: another
    /// machine may have started them meanwhile.
    #[serde(skip)]
    pub(crate) on: bool,
    /// The automatic sessions followed until they end, by issue.
    pub(crate) running: BTreeMap<u64, Auto>,
    /// The refused starts of each issue not started yet: none after
    /// `ATTEMPTS`.
    pub(crate) failed: BTreeMap<u64, u32>,
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
/// given up, not assigned when pickup was turned on and with no `live`
/// session starts,
/// lowest number first, while fewer than `limit` automatic sessions are
/// live; the rest wait.
pub(crate) fn due(p: &Pickup, assigned: &[u64], live: &BTreeSet<u64>, limit: Option<u32>) -> Plan {
    let new: BTreeSet<u64> = assigned
        .iter()
        .copied()
        .filter(|n| !p.started.contains(n) && !p.seen_at_enable.contains(n) && !live.contains(n))
        .filter(|n| p.failed.get(n).is_none_or(|failed| *failed < ATTEMPTS))
        .collect();
    let mut new: Vec<u64> = new.into_iter().collect();
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

/// Whether `url` is the web page of issue `number` of `repo`, as the forge
/// lists it: on its host, ending in `/<org>/<name>/issues/<number>`.
pub(crate) fn issue_of_repo(repo: &TeamRepo, number: u64, url: &str) -> bool {
    let url = url.to_lowercase();
    let Some(rest) = ["https://", "http://"]
        .iter()
        .find_map(|scheme| url.strip_prefix(scheme))
    else {
        return false;
    };
    let (at, path) = rest.split_once('/').unwrap_or((rest, ""));
    let at = at.rsplit_once(':').map_or(at, |(at, _)| at);

    at == repo.hostname() && format!("/{path}").ends_with(&format!("/{}/issues/{number}", repo.repo))
}

/// Snapshots the issues `assigned` now as the backlog the first round
/// pickup is on, then plans the round as `due` does with the live sessions
/// of `cards`.
fn plan(state: &mut Pickup, assigned: &[u64], cards: &[Card], limit: Option<u32>) -> Plan {
    if !state.on {
        state.on = true;
        state.seen_at_enable = assigned.iter().copied().collect();
    }

    due(state, assigned, &live(state, cards), limit)
}

/// The issues of `cards` with a session that has not ended: those its
/// branch or its title (`#<n> `, as automatic sessions are titled) names,
/// and those of the automatic sessions in `p`.
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
        .flat_map(|card| {
            let titled = issue_of_title(&card.title);
            card.issue.into_iter().chain(titled).chain(autos(card))
        })
        .collect()
}

/// The issue a title that starts `#<n> ` names.
fn issue_of_title(title: &str) -> Option<u64> {
    let (number, _) = title.strip_prefix('#')?.split_once(' ')?;

    match number.bytes().all(|b| b.is_ascii_digit()) {
        true => number.parse().ok(),
        false => None,
    }
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
    /// The machines that start sessions when this one runs no agent, best
    /// first.
    pub(super) elsewhere: Vec<EndpointId>,
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

    /// Notes that pickup is off for `repo`: its state goes from memory, so
    /// turning it on again leaves the issues assigned by then alone.
    pub(super) fn off(&mut self, repo: &TeamRepo) {
        self.queued.remove(repo);
        self.states.remove(repo);
    }

    /// Notes pickup off for each team this machine no longer acts for.
    pub(super) fn keep(&mut self, held: &BTreeSet<TeamRepo>) {
        let gone: Vec<TeamRepo> = self
            .states
            .keys()
            .filter(|repo| !held.contains(*repo))
            .cloned()
            .collect();

        for repo in gone {
            self.off(&repo);
        }
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

        ended(&round, &mut state).await;
        let numbers: Vec<u64> = assigned.iter().map(|issue| issue.number).collect();
        let plan = plan(&mut state, &numbers, round.cards, limit);
        let issue = |n: &u64| assigned.iter().find(|issue| issue.number == *n);
        let starts = plan
            .start
            .iter()
            .filter_map(issue)
            .map(|issue| start(&round, issue));

        for (issue, started) in plan.start.iter().zip(join_all(starts).await) {
            match &started {
                Ok(_) => {}
                Err(Unstarted::Unknown(e)) => {
                    log::warn!("a session on {repo}#{issue} may have started: {e}");
                }
                Err(Unstarted::Refused(e)) => {
                    log::info!("no session started on {repo}#{issue}: {e}");
                }
            }

            record(&mut state, *issue, started, |title| {
                needs_you(&round, *issue, title)
            });
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
    let absent = off_the_board(state, round.cards);
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

    report_ended(state, &ended, &heads, |n, title| needs_you(round, n, title));
}

/// Stops following the `ended` automatic sessions, each with a Needs you
/// card, put by `needs_you`, unless a pull request has its branch among
/// `heads`. One whose card finds no room is followed until the next round.
fn report_ended(
    state: &mut Pickup,
    ended: &[u64],
    heads: &[String],
    mut needs_you: impl FnMut(u64, String) -> bool,
) {
    for n in ended {
        let Some(auto) = state.running.get(n) else {
            continue;
        };
        let settled = heads.contains(&auto.branch)
            || needs_you(*n, format!("#{n} ended without a pull request"));

        if settled {
            state.running.remove(n);
        }
    }
}

/// Notes how the start of a session on issue `n` went. Its start refused
/// for the `ATTEMPTS`th time, pickup gives it up with a Needs you card,
/// put by `needs_you`; with no room for the card, it gets one more try.
fn record(
    state: &mut Pickup,
    n: u64,
    started: Result<Auto, Unstarted>,
    needs_you: impl FnOnce(String) -> bool,
) {
    match started {
        Ok(auto) => {
            state.running.insert(n, auto);
        }
        Err(Unstarted::Unknown(_)) => {}
        Err(Unstarted::Refused(reason)) => {
            let failed = state.failed.entry(n).or_default();
            *failed += 1;

            if *failed >= ATTEMPTS && !needs_you(format!("#{n} could not start: {reason}")) {
                *failed = ATTEMPTS - 1;
            }

            return;
        }
    }

    state.failed.remove(&n);
    state.started.insert(n);
}

/// Puts the Needs you card `title` of issue `n` on this mesh's apps;
/// whether there was room for it.
fn needs_you(round: &Round<'_>, n: u64, title: String) -> bool {
    let card = Card {
        id: format!("{PICKUP_CARD}{n}"),
        title: title.chars().take(TITLE).collect(),
        lane: Lane::NeedsYou {},
        login: round.team.mine.login.clone(),
        machine: String::new(),
        issue: Some(n),
        pr: None,
        updated_at: SystemTime::now(),
    };

    round.link.with(|teams| {
        let cards = teams.asks.cards.entry(round.team.repo.clone()).or_default();
        add_card(cards, card)
    })
}

/// Adds `card` to `cards` in place of the one with its id; whether there
/// was room. A full list drops its oldest pickup card for it, never a
/// teammate's ask.
fn add_card(cards: &mut Vec<Card>, card: Card) -> bool {
    cards.retain(|old| old.id != card.id);

    if cards.len() >= MAX_CARDS {
        let pickups = cards.iter().enumerate();
        let oldest = pickups
            .filter(|(_, old)| old.id.starts_with(PICKUP_CARD))
            .min_by_key(|(_, old)| old.updated_at)
            .map(|(at, _)| at);
        let Some(at) = oldest else {
            return false;
        };
        cards.remove(at);
    }

    cards.push(card);
    true
}

/// The followed automatic sessions, by issue, whose card is not among
/// `cards`, with that card's id.
fn off_the_board(state: &Pickup, cards: &[Card]) -> Vec<(u64, String)> {
    state
        .running
        .iter()
        .filter(|(_, auto)| !cards.iter().any(|card| card.id == auto.card))
        .map(|(n, auto)| (*n, auto.card.clone()))
        .collect()
}

/// Starts a session on `issue` here, when this machine runs an agent, or
/// on the first of the machines the round names that does not refuse it,
/// as one with no agent does.
async fn start(round: &Round<'_>, issue: &OpenIssue) -> Result<Auto, Unstarted> {
    let order = PickupStart {
        repo: round.team.repo.clone(),
        number: issue.number,
        title: issue.title.clone(),
        url: issue.url.clone(),
    };

    if round.project.checkout.is_some() && round.agents.auto_harness().is_some() {
        return match timeout(START_LIMIT, start_issue(round.agents, round.mesh, &order)).await {
            Ok(started) => started.map_err(Unstarted::Refused),
            Err(_) => Err(Unstarted::Unknown(
                "the agent did not start in time".to_owned(),
            )),
        };
    }

    let args = serde_json::to_string(&order).map_err(|e| Unstarted::Refused(e.to_string()))?;
    let request = Request {
        session: SessionId::default(),
        tool: Tool::TeamPickup,
        args,
        approved: Approved::No,
    };
    let mut refused = "no machine of the mesh with the project runs an agent".to_owned();

    for machine in &round.elsewhere {
        let answer = timeout(START_LIMIT, over_the_mesh(round.mesh, *machine, &request)).await;

        match started_there(answer.ok()) {
            Err(Unstarted::Refused(e)) => refused = e,
            started => return started,
        }
    }

    Err(Unstarted::Refused(refused))
}

/// How a start another machine was asked for went, by its `answer`; `None`
/// when it did not answer in time. A call that never reached the machine
/// is refused, to be tried again; one that broke after it went out may
/// have started the session.
fn started_there(answer: Option<Result<Reply, CallFailed>>) -> Result<Auto, Unstarted> {
    match answer {
        Some(Ok(Reply::Done {
            text,
            failed: false,
        })) => serde_json::from_str(&text).map_err(|e| Unstarted::Unknown(e.to_string())),
        Some(Ok(Reply::Done { text, .. })) => Err(Unstarted::Refused(text)),
        Some(Ok(other)) => Err(Unstarted::Unknown(format!("it answered {other:?}"))),
        Some(Err(CallFailed::Unsent(e))) => Err(Unstarted::Refused(format!("{e:#}"))),
        Some(Err(CallFailed::Sent(e))) => Err(Unstarted::Unknown(format!("{e:#}"))),
        None => Err(Unstarted::Unknown("it did not answer in time".to_owned())),
    }
}

#[cfg(test)]
#[path = "../tests/team_pickup.rs"]
mod tests;
