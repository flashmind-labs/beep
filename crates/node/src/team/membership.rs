//! This mesh's place on the teams its gateway acts for: its roster entry
//! names the mesh's machines with the project as they come and go, and once
//! a day the forge is asked whether this mesh and every member of another
//! mesh may still push.

use super::{
    home::{Home, Project},
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
use tokio::time::Instant;
use tokio_util::task::AbortOnDropHandle;

/// How often every member is checked on the forge.
const UPKEEP: Duration = Duration::from_secs(24 * 60 * 60);
/// The least time between two announcements of this mesh's devices on a team.
const ANNOUNCE: Duration = Duration::from_secs(60);
/// How often an announcement goes again to the teammates it did not reach.
const RESEND: Duration = Duration::from_secs(10 * 60);

/// A team this machine may act for.
#[derive(Clone)]
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
pub(super) fn teams(home: &Home, teams: &Teams) -> Vec<Team> {
    let own = home.mesh;

    home.projects
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
#[derive(Default)]
pub(super) struct Membership {
    /// The last time this mesh's entries were sent on each team.
    announced: BTreeMap<TeamRepo, Announced>,
    /// When each held team's members were last checked.
    checked: BTreeMap<TeamRepo, Instant>,
}

/// This mesh's entry and the members it admitted, as they were sent to the
/// teammates, and whom they did not reach yet.
struct Announced {
    entry: Member,
    admitted: Vec<Member>,
    /// The teammates it was sent to.
    to: Vec<Member>,
    /// When it was first sent, and last.
    since: Instant,
    at: Instant,
    /// The send under way; it ends with the meshes it did not reach.
    sending: Option<AbortOnDropHandle<BTreeSet<EndpointId>>>,
    unsent: BTreeSet<EndpointId>,
}

impl Announced {
    fn sent(
        link: &TeamLink,
        repo: &TeamRepo,
        entry: Member,
        admitted: Vec<Member>,
        to: Vec<Member>,
    ) -> Self {
        let now = Instant::now();
        let mut announced = Self {
            sending: None,
            entry,
            admitted,
            to,
            since: now,
            at: now,
            unsent: BTreeSet::new(),
        };
        announced.sending = Some(send(link, announced.roster(repo), announced.to.clone()));

        announced
    }

    /// The roster it sends: this mesh's entry, then the members it admitted.
    fn roster(&self, repo: &TeamRepo) -> TeamMsg {
        let members = [std::slice::from_ref(&self.entry), &self.admitted].concat();

        roster(repo, members)
    }

    /// Whether some teammate may still not have it.
    fn open(&self) -> bool {
        self.sending.is_some() || !self.unsent.is_empty()
    }
}

impl Membership {
    /// Forgets the checks of the teams that are not `held` here any more.
    pub(super) fn keep(&mut self, held: &BTreeSet<TeamRepo>) {
        self.checked.retain(|repo, _| held.contains(repo));
    }

    /// Sends this mesh's entry on each of `teams`, the teams held here: the
    /// entry as written when this machine starts holding one, and the
    /// entry naming the mesh's machines with the project while the written
    /// one does not (taken here too, for a desktop to write), once per change
    /// and at most every `ANNOUNCE`. The members this mesh admitted go with
    /// it, from the record as it is, and again whenever they change: only
    /// the mesh that admitted a member vouches for them, and a teammate
    /// offline at the join takes them later. Every `RESEND`, a send goes
    /// again to the teammates it did not reach, for a day, even once the
    /// team is held elsewhere: a teammate only takes it from a device it
    /// knows. `online` and `member` say which machines are online and in the
    /// mesh.
    pub(super) fn announce(
        &mut self,
        link: &TeamLink,
        online: &BTreeSet<EndpointId>,
        member: impl Fn(EndpointId) -> bool,
        teams: &[Team],
        projects: &BTreeMap<TeamRepo, Project>,
    ) {
        for last in self.announced.values_mut() {
            if let Some(sent) = last.sending.take_if(|sending| sending.is_finished()) {
                last.unsent = sent.now_or_never().and_then(Result::ok).unwrap_or_default();
            }
        }

        let held = |repo: &TeamRepo| teams.iter().any(|team| team.repo == *repo);
        self.announced
            .retain(|repo, last| held(repo) || (last.open() && last.since.elapsed() < UPKEEP));

        for team in teams {
            let devices = &projects[&team.repo].devices;
            let fresh = with_devices(&team.mine, devices, online, &member);
            let admitted: Vec<Member> = team
                .others
                .iter()
                .filter(|member| member.admitted_by == Some(team.mine.mesh))
                .cloned()
                .collect();
            let last = self.announced.get(&team.repo);
            let new = match (&fresh, last) {
                (_, None) => Some(fresh.clone().unwrap_or_else(|| team.mine.clone())),
                (Some(entry), Some(last))
                    if ids(&last.entry.devices) != ids(&entry.devices)
                        && last.at.elapsed() >= ANNOUNCE =>
                {
                    Some(entry.clone())
                }
                (_, Some(last)) if last.admitted != admitted => Some(last.entry.clone()),
                _ => None,
            };

            if let Some(entry) = new {
                if fresh.as_ref() == Some(&entry) {
                    take(link, team, &entry);
                }
                let others = team.others.clone();
                let announced = Announced::sent(link, &team.repo, entry, admitted, others);
                self.announced.insert(team.repo.clone(), announced);
                continue;
            }

            if let Some(last) = self.announced.get_mut(&team.repo) {
                last.to = team.others.clone();
            }
        }

        for (repo, last) in &mut self.announced {
            if last.sending.is_some() || last.unsent.is_empty() || last.at.elapsed() < RESEND {
                continue;
            }

            let to = last
                .to
                .iter()
                .filter(|member| last.unsent.contains(&member.mesh));
            last.sending = Some(send(link, last.roster(repo), to.cloned().collect()));
            last.unsent.clear();
            last.at = Instant::now();
        }
    }

    /// The held `teams` whose members are due for the daily check on the
    /// forge (`check_members`), marked checked now.
    pub(super) fn due(&mut self, teams: &[Team]) -> Vec<Team> {
        let due: Vec<Team> = teams
            .iter()
            .filter(|team| {
                let checked = self.checked.get(&team.repo);
                checked.is_none_or(|at| at.elapsed() >= UPKEEP)
            })
            .cloned()
            .collect();

        for team in &due {
            self.checked.insert(team.repo.clone(), Instant::now());
        }

        due
    }
}

fn ids(devices: &[EndpointAddr]) -> BTreeSet<EndpointId> {
    devices.iter().map(|device| device.id).collect()
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
/// teammate's roster is taken, for a desktop to write. It comes as from a
/// device on the entry: this machine is not one when it acts for a team
/// whose entry names no machine with the project any more.
fn take(link: &TeamLink, team: &Team, entry: &Member) {
    let roster = roster(&team.repo, vec![entry.clone()]);
    let own = Some(team.mine.mesh);
    let Some(from) = team.mine.devices.first() else {
        return;
    };

    match link.with(|link| link.rosters.apply(from.id, own, &roster)) {
        Ok(applied) if applied.changed.is_some() => link.wake(),
        Ok(_) => {}
        Err(refusal) => log::info!(
            "this mesh's devices on {} did not change: {refusal}",
            team.repo
        ),
    }
}

/// Sends `roster` to each of `to` in the background; the send ends with the
/// meshes of those it did not reach.
fn send(
    link: &TeamLink,
    roster: TeamMsg,
    to: Vec<Member>,
) -> AbortOnDropHandle<BTreeSet<EndpointId>> {
    let link = link.clone();

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

fn roster(repo: &TeamRepo, members: Vec<Member>) -> TeamMsg {
    TeamMsg::Roster {
        repo: repo.clone(),
        members,
    }
}

/// Once a day for each held team, asks the forge whether this mesh's own
/// login on `team` may still push, and leaves the team when not: the forge tells anyone what they may do
/// themselves, so this catches what a member who is not the repository's
/// admin cannot see of others. Then asks it of each member of another mesh,
/// and removes those who may not. Only a definite no takes anyone off.
pub(super) async fn check_members(link: &TeamLink, team: &Team) {
    if link.may_push(&team.repo, &team.mine.login).await == Some(false) {
        remove(link, team, &team.mine.login);
        return;
    }

    for member in &team.others {
        if link.may_push(&team.repo, &member.login).await == Some(false) {
            remove(link, team, &member.login);
        }
    }
}

/// Takes `login` off `team` here and tells every other member: a `Leave`
/// for this mesh's own login, a `Remove` for another's.
fn remove(link: &TeamLink, team: &Team, login: &ForgeLogin) {
    let (repo, login) = (team.repo.clone(), login.clone());
    let leaving = login == team.mine.login;

    if leaving {
        log::info!("{login} may no longer push to {repo}: leaving the team");
    } else {
        log::info!("{login} may no longer push to {repo}: removing them from the team");
    }

    if link
        .with(|link| link.rosters.remove_member(&repo, &login))
        .is_some()
    {
        link.wake();
    }

    let msg = match leaving {
        true => TeamMsg::Leave { repo, login },
        false => TeamMsg::Remove { repo, login },
    };
    link.tell(team.others.clone(), msg);
}

#[cfg(test)]
#[path = "../tests/team_membership.rs"]
mod tests;
