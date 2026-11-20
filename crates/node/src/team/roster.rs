//! The teams this mesh is on, as the link checks them: who may say what
//! about which repository. The mesh's records come from its desktops; a
//! change this device takes while it cannot write them stays pending until
//! the mesh moves on.

use beep_proto::team::{ForgeLogin, Member, Removed, TeamMsg, TeamRecord, TeamRepo};
use iroh::EndpointId;
use std::collections::BTreeMap;

/// Why a message about a team was turned away.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum Refusal {
    #[error("Not a member of {0}")]
    NotMember(TeamRepo),
    #[error("Only {0} can leave as {0}")]
    NotLeaver(ForgeLogin),
    #[error("Not on a team with this device")]
    Stranger,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Rosters {
    /// As the mesh holds them.
    records: BTreeMap<TeamRepo, TeamRecord>,
    /// Changed here and not written yet: these count over `records`.
    pending: BTreeMap<TeamRepo, TeamRecord>,
}

impl Rosters {
    /// Takes the mesh's records. A pending change gives way once the mesh's
    /// record of its repo is written: as the change, or as a desktop's
    /// own merge of it.
    pub(crate) fn refresh(&mut self, records: BTreeMap<TeamRepo, TeamRecord>) {
        let old = std::mem::replace(&mut self.records, records);

        self.pending.retain(|repo, pending| {
            let now = self.records.get(repo);
            now == old.get(repo) && now != Some(pending)
        });
    }

    pub(crate) fn get(&self, repo: &TeamRepo) -> Option<&TeamRecord> {
        self.pending.get(repo).or_else(|| self.records.get(repo))
    }

    /// The changes waiting for a desktop to write them.
    pub(crate) fn pending(&self) -> Vec<TeamRecord> {
        self.pending.values().cloned().collect()
    }

    /// Whether `peer` is a device of a member of `repo`'s team.
    pub(crate) fn allows(&self, peer: EndpointId, repo: &TeamRepo) -> bool {
        self.get(repo)
            .is_some_and(|record| member_of(record, peer).is_some())
    }

    /// Whether `peer` is a device of a member of any team.
    pub(crate) fn knows(&self, peer: EndpointId) -> bool {
        self.records
            .keys()
            .chain(self.pending.keys())
            .any(|repo| self.allows(peer, repo))
    }

    /// Takes `msg` from the device `from`: the record it changed, `None`
    /// when it changed nothing, or why it was refused.
    pub(crate) fn apply(
        &mut self,
        from: EndpointId,
        msg: &TeamMsg,
    ) -> Result<Option<TeamRecord>, Refusal> {
        let Some(repo) = repo_of(msg) else {
            return match self.knows(from) {
                true => Ok(None),
                false => Err(Refusal::Stranger),
            };
        };
        let record = self
            .get(repo)
            .ok_or_else(|| Refusal::NotMember(repo.clone()))?;
        let sender = member_of(record, from).ok_or_else(|| Refusal::NotMember(repo.clone()))?;
        let mut next = record.clone();

        match msg {
            TeamMsg::Roster { members, .. } => {
                for member in members {
                    take(&mut next, member, sender.mesh);
                }
            }
            TeamMsg::Leave { login, .. } if *login != sender.login => {
                return Err(Refusal::NotLeaver(login.clone()));
            }
            TeamMsg::Leave { login, .. } | TeamMsg::Remove { login, .. } => {
                remove(&mut next, login);
            }
            _ => return Ok(None),
        }

        Ok(self.change(next))
    }

    /// Adds `member`, admitted by this device through a join: their entry
    /// as they gave it now.
    pub(crate) fn admit(&mut self, repo: &TeamRepo, member: Member) -> Option<TeamRecord> {
        let mut next = self.get(repo)?.clone();
        next.members
            .retain(|have| (&have.login, have.mesh) != (&member.login, member.mesh));
        next.members.push(member);

        self.change(next)
    }

    /// Keeps `next` pending when it differs from what counts now.
    fn change(&mut self, next: TeamRecord) -> Option<TeamRecord> {
        if self.get(&next.repo) == Some(&next) {
            return None;
        }

        self.pending.insert(next.repo.clone(), next.clone());
        Some(next)
    }
}

/// A team record as it starts: these members, no invites, the defaults.
pub(crate) fn record(repo: TeamRepo, members: Vec<Member>) -> TeamRecord {
    TeamRecord {
        repo,
        members,
        invites: Vec::new(),
        pickup: true,
        pickup_limit: None,
        asks: true,
        removed: Vec::new(),
    }
}

/// Whether `login` in `mesh` was removed from `record`'s team.
pub(crate) fn was_removed(record: &TeamRecord, login: &ForgeLogin, mesh: EndpointId) -> bool {
    record
        .removed
        .iter()
        .any(|gone| gone.login == *login && gone.mesh == mesh)
}

/// The member whose devices `peer` is one of.
fn member_of(record: &TeamRecord, peer: EndpointId) -> Option<&Member> {
    record
        .members
        .iter()
        .find(|member| member.devices.iter().any(|device| device.id == peer))
}

/// Takes one entry of a roster sent by a device of the member in `sender`:
/// any member adds a new one, only the member's own devices change theirs,
/// and nobody brings back one who was removed.
fn take(record: &mut TeamRecord, member: &Member, sender: EndpointId) {
    if was_removed(record, &member.login, member.mesh) {
        return;
    }

    let have = record
        .members
        .iter_mut()
        .find(|have| have.login == member.login && have.mesh == member.mesh);

    match have {
        Some(have) if member.mesh == sender => *have = member.clone(),
        Some(_) => {}
        None => record.members.push(member.clone()),
    }
}

fn remove(record: &mut TeamRecord, login: &ForgeLogin) {
    let (gone, kept) = std::mem::take(&mut record.members)
        .into_iter()
        .partition(|member| member.login == *login);
    record.members = kept;

    for member in gone {
        let removed = Removed {
            login: member.login,
            mesh: member.mesh,
        };
        record.removed.push(removed);
    }
}

fn repo_of(msg: &TeamMsg) -> Option<&TeamRepo> {
    match msg {
        TeamMsg::Join { repo, .. }
        | TeamMsg::Roster { repo, .. }
        | TeamMsg::Invite { repo, .. }
        | TeamMsg::Subscribe { repo }
        | TeamMsg::Board { repo, .. }
        | TeamMsg::BoardChange { repo, .. }
        | TeamMsg::Ask { repo, .. }
        | TeamMsg::Leave { repo, .. }
        | TeamMsg::Remove { repo, .. } => Some(repo),
        TeamMsg::Challenge { .. }
        | TeamMsg::Proved { .. }
        | TeamMsg::Reply { .. }
        | TeamMsg::Refused { .. } => None,
    }
}
