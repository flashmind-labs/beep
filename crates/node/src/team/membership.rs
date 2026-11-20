//! This mesh's place on the teams its gateway acts for: its roster entry
//! names the mesh's machines with the project as they come and go, and once
//! a day every member of another mesh is checked on the forge.

use super::{
    gateway::{FORGE_LIMIT, Team},
    home::Project,
    link::TeamLink,
    roster::MAX_DEVICES,
};
use crate::machine::elect::online;
use beep_mesh::Mesh;
use beep_proto::team::{ForgeLogin, Member, TeamMsg, TeamRepo};
use iroh::{EndpointAddr, EndpointId};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};
use tokio::time::{Instant, timeout};

/// How often every member is checked on the forge.
const UPKEEP: Duration = Duration::from_secs(24 * 60 * 60);
/// The least time between two announcements of this mesh's devices on a team.
const ANNOUNCE: Duration = Duration::from_secs(60);

/// What the gateway keeps of the membership of the teams it holds.
#[derive(Default)]
pub(super) struct Membership {
    /// The devices last announced on each held team, and when.
    announced: BTreeMap<TeamRepo, (BTreeSet<EndpointId>, Instant)>,
    /// When each held team's members were last checked.
    checked: BTreeMap<TeamRepo, Instant>,
}

impl Membership {
    /// Forgets the teams that are not `held` here any more.
    pub(super) fn keep(&mut self, held: &BTreeSet<TeamRepo>) {
        self.checked.retain(|repo, _| held.contains(repo));
        self.announced.retain(|repo, _| held.contains(repo));
    }

    /// Names this mesh's machines with each held project on its entry, when
    /// they changed: once per change, at most every `ANNOUNCE`.
    pub(super) fn announce(
        &mut self,
        link: &TeamLink,
        mesh: &Mesh,
        me: EndpointId,
        teams: &[Team],
        projects: &BTreeMap<TeamRepo, Project>,
    ) {
        let mut online = online(mesh);
        online.insert(me);

        for team in teams {
            let devices = &projects[&team.repo].devices;
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
                announce(link, me, team, entry);
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

    let ids = |devices: &[EndpointAddr]| devices.iter().map(|device| device.id).collect::<BTreeSet<_>>();
    let changed = ids(&devices) != ids(&mine.devices);

    changed.then(|| Member {
        devices,
        ..mine.clone()
    })
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

#[cfg(test)]
#[path = "../tests/team_membership.rs"]
mod tests;
