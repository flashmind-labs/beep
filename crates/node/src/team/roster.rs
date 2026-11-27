//! The teams this mesh is on, as the link checks them: who may say what
//! about which repository. The mesh's records come from its desktops; a
//! change this device takes while it cannot write them stays pending until
//! the mesh's record holds it.

use beep_proto::team::{ForgeLogin, Member, TeamMsg, TeamRecord, TeamRepo};
use iroh::EndpointId;
use std::collections::BTreeMap;

/// The most devices a member names, and addresses a device carries.
pub(crate) const MAX_DEVICES: usize = 32;
pub(crate) const MAX_ADDRS: usize = 8;

/// Why a message about a team was turned away.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum Refusal {
    #[error("Not a member of {0}")]
    NotMember(TeamRepo),
    #[error("Only {0} can leave as {0}")]
    NotLeaver(ForgeLogin),
    #[error("Not on a team with this device")]
    Stranger,
    #[error("{0} was removed from this team")]
    Removed(ForgeLogin),
    #[error("Another member already has that login, mesh or device")]
    Taken,
    #[error("This mesh is on the team already")]
    OwnMesh,
    #[error("A member names 1 to 32 devices, each with at most 8 addresses")]
    Devices,
    #[error("The joining device is not among the devices it named")]
    NotThePeer,
}

/// What a message changed at once, and the members it adds once the forge
/// says they may push.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Applied {
    pub(crate) changed: Option<TeamRecord>,
    pub(crate) newcomers: Vec<Member>,
}

/// What an entry is to a record, by the rules every member applies: a mesh,
/// a login and a device each belong to one member, and only a member's own
/// devices change their entry.
enum Vetted {
    /// Not on the team yet.
    New,
    /// The sender's own entry, changed.
    Own(usize),
    /// Another member's entry, as the sender has it: theirs to change.
    Other,
    /// Already just so.
    Same,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Rosters {
    /// As the mesh holds them.
    records: BTreeMap<TeamRepo, TeamRecord>,
    /// Changed here and not written yet: these count over `records`.
    pending: BTreeMap<TeamRepo, TeamRecord>,
}

impl Rosters {
    /// Takes the mesh's records. A pending change moves onto the record
    /// written since, whatever else changed in it, and gives way once the
    /// record holds all of it.
    pub(crate) fn refresh(&mut self, records: BTreeMap<TeamRepo, TeamRecord>) {
        let old = std::mem::replace(&mut self.records, records);

        for (repo, pending) in std::mem::take(&mut self.pending) {
            let Some(now) = self.records.get(&repo) else {
                continue;
            };
            let mut next = now.clone();
            next.merge(match old.get(&repo) {
                Some(base) => pending.since(base),
                None => pending,
            });

            if next != *now {
                self.pending.insert(repo, next);
            }
        }
    }

    /// `repo`'s record as the mesh holds it, without a change pending here:
    /// what every machine of the mesh reads alike.
    pub(crate) fn written(&self, repo: &TeamRepo) -> Option<&TeamRecord> {
        self.records.get(repo)
    }

    pub(crate) fn get(&self, repo: &TeamRepo) -> Option<&TeamRecord> {
        self.pending.get(repo).or_else(|| self.records.get(repo))
    }

    /// The changes waiting for a desktop to write them: what each pending
    /// record changed from the mesh's.
    pub(crate) fn pending(&self) -> Vec<TeamRecord> {
        let changes = self
            .pending
            .iter()
            .map(|(repo, pending)| match self.records.get(repo) {
                Some(written) => pending.since(written),
                None => pending.clone(),
            });

        changes.collect()
    }

    /// Whether `peer` is a device of a member of `repo`'s team.
    pub(crate) fn allows(&self, peer: EndpointId, repo: &TeamRepo) -> bool {
        self.get(repo)
            .is_some_and(|record| sender_of(record, peer).is_some())
    }

    /// Whether `peer` is a device of a member of any team.
    pub(crate) fn knows(&self, peer: EndpointId) -> bool {
        self.records
            .keys()
            .chain(self.pending.keys())
            .any(|repo| self.allows(peer, repo))
    }

    /// Takes `msg` from the device `from`, in a mesh other than `own`: what
    /// it changed and whom it adds, or why it was refused.
    pub(crate) fn apply(
        &mut self,
        from: EndpointId,
        own: Option<EndpointId>,
        msg: &TeamMsg,
    ) -> Result<Applied, Refusal> {
        let Some(repo) = repo_of(msg) else {
            return match self.knows(from) {
                true => Ok(Applied::default()),
                false => Err(Refusal::Stranger),
            };
        };
        let record = self
            .get(repo)
            .ok_or_else(|| Refusal::NotMember(repo.clone()))?;
        let sender = sender_of(record, from).ok_or_else(|| Refusal::NotMember(repo.clone()))?;
        let mut next = record.clone();
        let mut newcomers = Vec::new();

        match msg {
            TeamMsg::Roster { members, .. } => {
                for entry in members {
                    if removes(&next, entry) {
                        continue;
                    }

                    // Only the mesh that admitted a newcomer vouches for
                    // them. The founder, admitted by nobody, every member
                    // has from its own join.
                    let vouched = entry.admitted_by == Some(next.members[sender].mesh);

                    match vet(&next, entry, Some(sender))? {
                        Vetted::New if Some(entry.mesh) == own => return Err(Refusal::OwnMesh),
                        Vetted::New if vouched => newcomers.push(entry.clone()),
                        Vetted::New => {}
                        Vetted::Own(at) => next.members[at] = entry.clone(),
                        Vetted::Other | Vetted::Same => {}
                    }
                }
            }
            TeamMsg::Leave { login, .. } if *login != next.members[sender].login => {
                return Err(Refusal::NotLeaver(login.clone()));
            }
            TeamMsg::Leave { login, .. } | TeamMsg::Remove { login, .. } => {
                next.remove(login);
            }
            _ => return Ok(Applied::default()),
        }

        let changed = self.change(next);
        Ok(Applied { changed, newcomers })
    }

    /// Adds `entry`, a newcomer in a roster from `from` that the forge
    /// vouched for, if the rules still let it in.
    pub(crate) fn add(
        &mut self,
        repo: &TeamRepo,
        from: EndpointId,
        own: Option<EndpointId>,
        entry: Member,
    ) -> Result<Option<TeamRecord>, Refusal> {
        let record = self
            .get(repo)
            .ok_or_else(|| Refusal::NotMember(repo.clone()))?;
        let sender = sender_of(record, from).ok_or_else(|| Refusal::NotMember(repo.clone()))?;

        if removes(record, &entry) {
            return Err(Refusal::Removed(entry.login));
        }
        if Some(entry.mesh) == own {
            return Err(Refusal::OwnMesh);
        }
        if !matches!(vet(record, &entry, Some(sender))?, Vetted::New) {
            return Ok(None);
        }

        // A removal of an earlier join of theirs ends.
        let mut next = record.clone();
        next.merge(TeamRecord::new(repo.clone(), vec![entry]));
        Ok(self.change(next))
    }

    /// Takes `login` off `repo`'s team, as a `Remove` from a member does: the
    /// record after it, when that changed anything.
    pub(crate) fn remove_member(
        &mut self,
        repo: &TeamRepo,
        login: &ForgeLogin,
    ) -> Option<TeamRecord> {
        let mut next = self.get(repo)?.clone();
        next.remove(login);

        self.change(next)
    }

    /// Whether a join by `entry` from the device `peer` may go ahead: the
    /// device is one it names, and it is new or the member it says it is.
    pub(crate) fn may_join(
        &self,
        repo: &TeamRepo,
        peer: EndpointId,
        own: Option<EndpointId>,
        entry: &Member,
    ) -> Result<(), Refusal> {
        self.vet_join(repo, peer, own, entry).map(drop)
    }

    /// Adds or updates `entry`, admitted by this device through a join from
    /// `peer`: the team's record after it.
    pub(crate) fn admit(
        &mut self,
        repo: &TeamRepo,
        peer: EndpointId,
        own: Option<EndpointId>,
        entry: Member,
    ) -> Result<TeamRecord, Refusal> {
        let (record, vetted) = self.vet_join(repo, peer, own, &entry)?;
        let mut next = record.clone();

        match vetted {
            Vetted::New => next.members.push(entry),
            Vetted::Own(at) => next.members[at] = entry,
            Vetted::Other | Vetted::Same => {}
        }

        Ok(self.change(next.clone()).unwrap_or(next))
    }

    fn vet_join(
        &self,
        repo: &TeamRepo,
        peer: EndpointId,
        own: Option<EndpointId>,
        entry: &Member,
    ) -> Result<(&TeamRecord, Vetted), Refusal> {
        let record = self
            .get(repo)
            .ok_or_else(|| Refusal::NotMember(repo.clone()))?;

        if !entry.devices.iter().any(|device| device.id == peer) {
            return Err(Refusal::NotThePeer);
        }
        if Some(entry.mesh) == own {
            return Err(Refusal::OwnMesh);
        }
        // Only a readmission here lets a removed member join again, however
        // late: a teammate's removal stands against an invite still open.
        let mut removed = record.removed.iter();
        if removed.any(|gone| gone.login == entry.login && gone.mesh == entry.mesh) {
            return Err(Refusal::Removed(entry.login.clone()));
        }

        match vet(record, entry, sender_of(record, peer))? {
            Vetted::Other => Err(Refusal::Taken),
            vetted => Ok((record, vetted)),
        }
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

/// Checks `members`, the roster an inviter answers a join with, by the rules
/// every member applies: each names 1 to 32 devices, and no two share a
/// login, a mesh or a device.
pub(crate) fn vet_roster(repo: &TeamRepo, members: &[Member]) -> Result<(), Refusal> {
    let mut record = TeamRecord::new(repo.clone(), Vec::new());

    for entry in members {
        match vet(&record, entry, None)? {
            Vetted::New => record.members.push(entry.clone()),
            _ => return Err(Refusal::Taken),
        }
    }

    Ok(())
}

/// Whether a removal on `record`'s team keeps `entry` off: one who joined
/// after it was let back in.
fn removes(record: &TeamRecord, entry: &Member) -> bool {
    record.removed.iter().any(|gone| gone.covers(entry))
}

/// The member whose devices `peer` is one of: one at most, since no two
/// members share a device.
fn sender_of(record: &TeamRecord, peer: EndpointId) -> Option<usize> {
    record
        .members
        .iter()
        .position(|member| member.devices.iter().any(|device| device.id == peer))
}

/// What `entry`, sent by a device of the member at `sender`, is to `record`.
fn vet(record: &TeamRecord, entry: &Member, sender: Option<usize>) -> Result<Vetted, Refusal> {
    let devices = &entry.devices;

    if devices.is_empty()
        || devices.len() > MAX_DEVICES
        || devices.iter().any(|device| device.addrs.len() > MAX_ADDRS)
    {
        return Err(Refusal::Devices);
    }
    let claimed: Vec<usize> = record
        .members
        .iter()
        .enumerate()
        .filter(|(_, member)| member.mesh == entry.mesh || member.login == entry.login)
        .map(|(at, _)| at)
        .collect();
    let at = match claimed[..] {
        [] => None,
        [at] if record.members[at].login == entry.login
            && record.members[at].mesh == entry.mesh =>
        {
            Some(at)
        }
        _ => return Err(Refusal::Taken),
    };
    let shares_a_device = record.members.iter().enumerate().any(|(other, member)| {
        Some(other) != at
            && member
                .devices
                .iter()
                .any(|have| devices.iter().any(|device| device.id == have.id))
    });

    if shares_a_device {
        return Err(Refusal::Taken);
    }

    Ok(match at {
        None => Vetted::New,
        Some(at) if record.members[at] == *entry => Vetted::Same,
        Some(at) if Some(at) == sender => Vetted::Own(at),
        Some(_) => Vetted::Other,
    })
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
