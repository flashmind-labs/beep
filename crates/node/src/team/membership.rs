//! This mesh's place on the teams its gateway acts for: its roster entry
//! names the mesh's machines with the project as they come and go, and once
//! a day every member of another mesh is checked on the forge.

use super::{
    gateway::FORGE_LIMIT,
    home::Project,
    link::{TeamLink, Teams},
    roster::MAX_DEVICES,
};
use beep_proto::team::{ForgeLogin, Member, TeamMsg, TeamRepo};
use futures::FutureExt;
use iroh::{EndpointAddr, EndpointId};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};
use tokio::time::{Instant, timeout};
use tokio_util::task::AbortOnDropHandle;

/// How often every member is checked on the forge.
const UPKEEP: Duration = Duration::from_secs(24 * 60 * 60);
/// The least time between two announcements of this mesh's devices on a team.
const ANNOUNCE: Duration = Duration::from_secs(60);
/// How often an announcement goes again to the teammates it did not reach.
const RESEND: Duration = Duration::from_secs(10 * 60);

/// A team this machine may act for.
pub(super) struct Team {
    pub(super) repo: TeamRepo,
    /// The mesh's name for the project.
    pub(super) project: String,
    /// This mesh's entry on it as the mesh holds it, which every machine of
    /// the mesh elects the gateway from alike.
    pub(super) mine: Member,
    /// The members of other meshes.
    pub(super) others: Vec<Member>,
}

/// The teams of this mesh's projects whose record the mesh holds.
pub(super) fn teams(teams: &Teams) -> Vec<Team> {
    let own = teams.home.mesh;

    teams
        .home
        .projects
        .iter()
        .filter_map(|(repo, project)| {
            let mine = teams
                .rosters
                .written(repo)?
                .members
                .iter()
                .find(|member| Some(member.mesh) == own)?
                .clone();
            let others = teams
                .rosters
                .get(repo)?
                .members
                .iter()
                .filter(|member| Some(member.mesh) != own)
                .cloned()
                .collect();
            Some(Team {
                repo: repo.clone(),
                project: project.name.clone(),
                mine,
                others,
            })
        })
        .collect()
}

/// What the gateway keeps of the membership of the teams it holds.
pub(super) struct Membership {
    me: EndpointId,
    /// The last announcement on each held team.
    announced: BTreeMap<TeamRepo, Announced>,
    /// When each held team's members were last checked.
    checked: BTreeMap<TeamRepo, Instant>,
}

/// This mesh's entry with new devices, as it was sent to the teammates.
struct Announced {
    entry: Member,
    at: Instant,
    /// The send under way; it ends with the meshes it did not reach.
    sending: Option<AbortOnDropHandle<BTreeSet<EndpointId>>>,
    unsent: BTreeSet<EndpointId>,
}

impl Membership {
    pub(super) fn new(me: EndpointId) -> Self {
        Self {
            me,
            announced: BTreeMap::new(),
            checked: BTreeMap::new(),
        }
    }

    /// Forgets the teams that are not `held` here any more.
    pub(super) fn keep(&mut self, held: &BTreeSet<TeamRepo>) {
        self.checked.retain(|repo, _| held.contains(repo));
        self.announced.retain(|repo, _| held.contains(repo));
    }

    /// Names this mesh's machines with each held project on its entry while
    /// the mesh's record does not: once per change, at most every `ANNOUNCE`.
    /// Every `RESEND`, the teammates a send did not reach get it again, and
    /// it is taken here again if the change was dropped before a desktop
    /// wrote it. `online` and `member` say which machines are online and in
    /// the mesh.
    pub(super) fn announce(
        &mut self,
        link: &TeamLink,
        online: &BTreeSet<EndpointId>,
        member: impl Fn(EndpointId) -> bool,
        teams: &[Team],
        projects: &BTreeMap<TeamRepo, Project>,
    ) {
        for team in teams {
            let devices = &projects[&team.repo].devices;
            let fresh = with_devices(&team.mine, devices, online, &member);
            let last = self.announced.get_mut(&team.repo);
            let new = fresh.as_ref().filter(|entry| {
                last.as_ref().is_none_or(|last| {
                    ids(&last.entry.devices) != ids(&entry.devices) && last.at.elapsed() >= ANNOUNCE
                })
            });

            if let Some(entry) = new {
                take(link, self.me, team, entry);
                let sending = send(link, team, entry, team.others.clone());
                let announced = Announced {
                    entry: entry.clone(),
                    at: Instant::now(),
                    sending: Some(sending),
                    unsent: BTreeSet::new(),
                };
                self.announced.insert(team.repo.clone(), announced);
                continue;
            }

            let Some(last) = last else { continue };

            if let Some(sent) = last.sending.take_if(|sending| sending.is_finished()) {
                last.unsent = sent.now_or_never().and_then(Result::ok).unwrap_or_default();
            }
            if last.sending.is_some() || last.at.elapsed() < RESEND {
                continue;
            }

            let needed = fresh.is_some_and(|entry| ids(&entry.devices) == ids(&last.entry.devices));
            let dropped = needed && !link.with(|link| holds(link, team, &last.entry));
            if dropped {
                take(link, self.me, team, &last.entry);
            }
            if !last.unsent.is_empty() {
                let to = team.others.iter().filter(|member| last.unsent.contains(&member.mesh));
                last.sending = Some(send(link, team, &last.entry, to.cloned().collect()));
                last.unsent.clear();
            }
            if dropped || last.sending.is_some() {
                last.at = Instant::now();
            }
        }
    }

    /// Once a day for each held team, asks the forge whether each member of
    /// another mesh may still push; one who may not is removed, here and on
    /// every other member's side. A failed check waits for the next day.
    pub(super) async fn upkeep(&mut self, link: &TeamLink, teams: &[Team]) {
        for team in teams {
            if self.checked.get(&team.repo).is_some_and(|at| at.elapsed() < UPKEEP) {
                continue;
            }

            self.checked.insert(team.repo.clone(), Instant::now());
            check_members(link, team).await;
        }
    }
}

fn ids(devices: &[EndpointAddr]) -> BTreeSet<EndpointId> {
    devices.iter().map(|device| device.id).collect()
}

/// Whether this device's view of `team` names `entry`'s devices.
fn holds(teams: &Teams, team: &Team, entry: &Member) -> bool {
    let record = teams.rosters.get(&team.repo);
    let mine = record.and_then(|record| record.members.iter().find(|member| member.mesh == entry.mesh));

    mine.is_some_and(|mine| ids(&mine.devices) == ids(&entry.devices))
}

/// `mine`, this mesh's entry, naming the devices it should: its own still in
/// the mesh (`member`), then the project's machines not on it yet, online
/// ones first, at most `MAX_DEVICES`. `None` when it names those already.
pub(crate) fn with_devices(
    mine: &Member,
    project: &[EndpointAddr],
    online: &BTreeSet<EndpointId>,
    member: impl Fn(EndpointId) -> bool,
) -> Option<Member> {
    let mut devices: Vec<EndpointAddr> = mine
        .devices
        .iter()
        .filter(|device| member(device.id))
        .cloned()
        .collect();
    let mut new: Vec<&EndpointAddr> = project
        .iter()
        .filter(|device| !devices.iter().any(|have| have.id == device.id))
        .collect();
    new.sort_by_key(|device| !online.contains(&device.id));
    devices.extend(new.into_iter().cloned());
    devices.truncate(MAX_DEVICES);

    let changed = ids(&devices) != ids(&mine.devices);

    changed.then(|| Member {
        devices,
        ..mine.clone()
    })
}

/// Takes `entry`, this mesh's entry on `team` with its devices now, as a
/// teammate's roster is taken, for a desktop to write.
fn take(link: &TeamLink, me: EndpointId, team: &Team, entry: &Member) {
    let roster = roster(team, entry);
    let own = Some(team.mine.mesh);

    match link.with(|link| link.rosters.apply(me, own, &roster)) {
        Ok(applied) if applied.changed.is_some() => link.wake(),
        Ok(_) => {}
        Err(refusal) => log::info!("this mesh's devices on {} did not change: {refusal}", team.repo),
    }
}

/// Sends `entry` to each of `to` in the background; the send ends with the
/// meshes of those it did not reach.
fn send(
    link: &TeamLink,
    team: &Team,
    entry: &Member,
    to: Vec<Member>,
) -> AbortOnDropHandle<BTreeSet<EndpointId>> {
    let (link, roster) = (link.clone(), roster(team, entry));

    AbortOnDropHandle::new(tokio::spawn(async move {
        let mut unsent = BTreeSet::new();

        for member in to {
            if let Err(e) = link.send(&member, &roster).await {
                log::info!("this mesh's devices did not reach {}: {e}", member.login);
                unsent.insert(member.mesh);
            }
        }

        unsent
    }))
}

fn roster(team: &Team, entry: &Member) -> TeamMsg {
    TeamMsg::Roster {
        repo: team.repo.clone(),
        members: vec![entry.clone()],
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

#[cfg(test)]
#[path = "../tests/team_membership.rs"]
mod tests;
