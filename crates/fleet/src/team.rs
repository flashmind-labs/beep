//! Teams as the desktop records them: a project's team record, its invites and
//! settings, joining and leaving, the contacts the teams bring, and assigning
//! issues. The team link is a node's: what it sends, and the cards only a
//! team's gateway holds, go through a node over `tools/1`.

use crate::{
    desk::Desk,
    error::{FleetError, Result},
    nearest_first,
    records::{find_project, projects, read, scan, write},
};
use beep_mesh::{JoinSecret, Key, TeamTicket, TeamWrites, keep_contacts, secret_hash};
use beep_projects::team_forge::{assign, my_login};
use beep_proto::{
    SessionId,
    node::tools::{
        self, Approved, Reply, Request, TeamAppArgs, TeamCards, TeamInviteArgs, TeamJoinArgs, Tool,
    },
    team::{ContactRecord, ForgeLogin, Member, OpenInvite, TeamRecord, TeamRepo},
};
use futures::future::join_all;
use iroh::{EndpointAddr, EndpointId};
use iroh_tickets::Ticket;
use serde::Serialize;
use std::time::{Duration, SystemTime};

/// How long an invite code lets people join.
const INVITE_FOR: Duration = Duration::from_secs(24 * 60 * 60);
/// How long a call that reaches another mesh may take, past the node's own
/// limits: a join (90 s with the forge login) or a contact's invite sent on
/// to the gateway (90 s).
const ACROSS: Duration = Duration::from_secs(100);
/// How long a call answered within this mesh may take, past a call the node
/// sends on to the gateway (30 s).
const WITHIN: Duration = Duration::from_secs(40);

impl Desk {
    /// The team of `project`, when it has one.
    pub fn team_record(&self, project: &str) -> Result<Option<TeamRecord>> {
        let repo = self.team_repo(project)?;

        Ok(read(&self.mesh, &Key::Team(repo)))
    }

    /// A new invite to `project`'s team, valid a day for anyone with push
    /// access: the `beepteam1` code. The first one starts the team, with
    /// this mesh on it.
    pub async fn team_invite(&self, project: &str) -> Result<String> {
        let repo = self.team_repo(project)?;
        let inviter = self.checkout_machine(project)?;
        let founder = match self.team_record(project)? {
            Some(record) if !self.on_team(&record) => return Err(removed()),
            Some(_) => None,
            None => Some(self.own_member(project).await?),
        };
        let secret = JoinSecret::generate();
        let invite = OpenInvite {
            secret_hash: secret_hash(&secret),
            expires_at: SystemTime::now() + INVITE_FOR,
        };

        {
            let _held = TeamWrites::lock().await;
            let mut record = match (self.team_record(project)?, founder) {
                (Some(record), _) if !self.on_team(&record) => return Err(removed()),
                (Some(record), _) => record,
                (None, Some(me)) => TeamRecord::new(repo.clone(), vec![me]),
                (None, None) => return Err(FleetError::refused("The team was left meanwhile")),
            };
            let now = SystemTime::now();
            record.invites.retain(|invite| invite.expires_at > now);
            record.invites.push(invite);
            write(&self.mesh, Key::Team(repo.clone()), Some(&record)).await?;
        }

        let ticket = TeamTicket {
            inviter,
            repo,
            secret,
        };
        Ok(ticket.encode_string())
    }

    /// The repository `ticket` invites to, and the project that has it here.
    pub fn team_ticket(&self, ticket: &str) -> Result<(TeamRepo, Option<String>)> {
        let read_ticket =
            TeamTicket::parse(ticket).map_err(|e| FleetError::refused(e.to_string()))?;
        let repo = read_ticket.repo;
        let project = projects(&self.mesh)
            .into_iter()
            .find(|stored| stored.record.url.parse::<TeamRepo>().as_ref() == Ok(&repo))
            .map(|stored| stored.name);

        Ok((repo, project))
    }

    /// Joins the team `ticket` invites to, proving push access from the
    /// nearest machine with the project, and records it and its contacts.
    pub async fn team_join(&self, ticket: &str) -> Result<TeamRecord> {
        let (repo, project) = self.team_ticket(ticket)?;
        let project = project.ok_or_else(|| FleetError::NotImported(repo.clone()))?;
        let machine = self.checkout_machine(&project)?;
        let args = TeamJoinArgs {
            ticket: ticket.trim().to_owned(),
        };
        let text = self.call(machine, Tool::TeamJoin, &args, ACROSS).await?;
        let joined: TeamRecord = serde_json::from_str(&text).map_err(anyhow::Error::from)?;

        let held = TeamWrites::lock().await;
        let record = match read::<TeamRecord>(&self.mesh, &Key::Team(repo.clone())) {
            // Settings and invites stay; nobody on the team is held off it.
            Some(mut kept) => {
                kept.removed.retain(|gone| {
                    let mut members = joined.members.iter();
                    !members.any(|member| member.login == gone.login && member.mesh == gone.mesh)
                });
                kept.members = joined.members;
                kept
            }
            None => joined,
        };
        write(&self.mesh, Key::Team(repo), Some(&record)).await?;
        keep_contacts(&self.mesh, &held).await?;

        Ok(record)
    }

    /// Takes `login` off `project`'s team, here and with every teammate.
    pub async fn team_remove(&self, project: &str, login: &ForgeLogin) -> Result<()> {
        let repo = self.team_repo(project)?;
        let record = self.team_record(project)?.ok_or(FleetError::NotFound)?;
        let member = record
            .members
            .iter()
            .find(|member| member.login == *login)
            .ok_or_else(|| FleetError::refused(format!("{login} is not on the team")))?;

        if member.mesh == self.mesh.id() {
            return Err(FleetError::refused("Leave the team instead"));
        }

        // Told first: the gateway tells the members its record still lists.
        let remove = TeamAppArgs::Remove {
            repo: repo.clone(),
            login: login.clone(),
        };
        self.team_app(&remove, WITHIN).await?;
        self.change_team(project, |record| record.remove(login))
            .await?;

        Ok(())
    }

    /// Leaves `project`'s team: the teammates are told, then its record and
    /// the contacts only it brought are gone, told or not.
    pub async fn team_leave(&self, project: &str) -> Result<()> {
        let repo = self.team_repo(project)?;

        if self.team_record(project)?.is_none() {
            return Ok(());
        }

        // Leaving is the user's own: a teammate not told finds out on their
        // next message, which this mesh refuses.
        let leave = TeamAppArgs::Leave { repo: repo.clone() };
        if let Err(e) = self.team_app(&leave, WITHIN).await {
            log::warn!("the teammates on {repo} were not told this mesh left: {e}");
        }

        let held = TeamWrites::lock().await;
        write::<TeamRecord>(&self.mesh, Key::Team(repo), None).await?;
        keep_contacts(&self.mesh, &held).await?;

        Ok(())
    }

    /// Lets `login` join `project`'s team again after they were removed or left.
    pub async fn team_readmit(&self, project: &str, login: &ForgeLogin) -> Result<()> {
        self.change_team(project, |record| {
            record.removed.retain(|gone| gone.login != *login);
        })
        .await
    }

    /// Whether issues assigned to the user start sessions on their own, at
    /// most how many at once, and whether teammates' agents may ask theirs.
    pub async fn team_settings(
        &self,
        project: &str,
        pickup: bool,
        pickup_limit: Option<u32>,
        asks: bool,
    ) -> Result<()> {
        self.change_team(project, |record| {
            record.pickup = pickup;
            record.pickup_limit = pickup_limit;
            record.asks = asks;
        })
        .await
    }

    /// Invites `contact` to `project`'s team: a new invite code reaches their
    /// devices, which show it with Accept.
    pub async fn team_invite_contact(&self, project: &str, contact: EndpointId) -> Result<()> {
        let repo = self.team_repo(project)?;
        let found: ContactRecord =
            read(&self.mesh, &Key::Contact(contact)).ok_or(FleetError::NotFound)?;

        if found.logins.iter().any(|(on, _)| *on == repo) {
            return Err(FleetError::refused("They are on this team already"));
        }

        // From a team shared with them, whose devices they know.
        let via = found
            .logins
            .iter()
            .map(|(on, _)| on)
            .find(|on| self.mesh.get(&Key::Team((*on).clone())).is_some())
            .cloned()
            .ok_or_else(|| FleetError::refused("No team is shared with them any more"))?;
        let ticket = self.team_invite(project).await?;
        let invite = TeamAppArgs::Invite {
            via,
            repo,
            ticket,
            devices: found.devices,
        };
        self.team_app(&invite, ACROSS).await?;

        Ok(())
    }

    /// The people of other meshes on this mesh's teams.
    pub fn contacts(&self) -> Vec<ContactRecord> {
        scan(&self.mesh, "contact/")
            .into_iter()
            .map(|(_, contact)| contact)
            .collect()
    }

    /// Removes `contact` from every team shared with them, telling each
    /// team, then forgets them. Like a leave, a team not told finds out on
    /// their next message, which this mesh refuses.
    pub async fn remove_contact(&self, contact: EndpointId) -> Result<()> {
        let found: ContactRecord =
            read(&self.mesh, &Key::Contact(contact)).ok_or(FleetError::NotFound)?;
        let tell = found.logins.iter().map(|(repo, login)| async move {
            let remove = TeamAppArgs::Remove {
                repo: repo.clone(),
                login: login.clone(),
            };
            if let Err(e) = self.team_app(&remove, WITHIN).await {
                log::warn!("the team of {repo} was not told {login} was removed: {e}");
            }
        });
        join_all(tell).await;

        let held = TeamWrites::lock().await;
        for (repo, login) in found.logins {
            let key = Key::Team(repo);
            let Some(mut record) = read::<TeamRecord>(&self.mesh, &key) else {
                continue;
            };
            record.remove(&login);
            write(&self.mesh, key, Some(&record)).await?;
        }
        write::<ContactRecord>(&self.mesh, Key::Contact(contact), None).await?;
        keep_contacts(&self.mesh, &held).await?;

        Ok(())
    }

    /// `project`'s Needs you cards and its teammates' cards, from the
    /// machine that acts for its team; none without a team.
    pub async fn team_cards(&self, project: &str) -> Result<TeamCards> {
        let repo = self.team_repo(project)?;

        if self.team_record(project)?.is_none() {
            return Ok(TeamCards::default());
        }

        let text = self.team_app(&TeamAppArgs::Cards { repo }, WITHIN).await?;

        Ok(serde_json::from_str(&text).map_err(anyhow::Error::from)?)
    }

    /// Drops `project`'s Needs you card `card`.
    pub async fn team_dismiss(&self, project: &str, card: &str) -> Result<()> {
        let repo = self.team_repo(project)?;
        self.team_record(project)?.ok_or(FleetError::NotFound)?;
        let dismiss = TeamAppArgs::Dismiss {
            repo,
            card: card.to_owned(),
        };
        self.team_app(&dismiss, WITHIN).await?;

        Ok(())
    }

    /// The contacts' invites to their teams that wait on this desktop.
    pub async fn team_invites(&self) -> Result<Vec<TeamInviteArgs>> {
        let text = self
            .call(self.local, Tool::TeamApp, &TeamAppArgs::Invites, WITHIN)
            .await?;

        Ok(serde_json::from_str(&text).map_err(anyhow::Error::from)?)
    }

    /// Drops a contact's invite, accepted or declined.
    pub async fn team_invite_drop(&self, invite: TeamInviteArgs) -> Result<()> {
        let drop = TeamAppArgs::DropInvite { invite };
        self.call(self.local, Tool::TeamApp, &drop, WITHIN).await?;

        Ok(())
    }

    /// Assigns `logins` to issue `number` of `project`; none assigns the user.
    pub async fn assign_issue(
        &self,
        project: &str,
        number: u64,
        logins: Vec<ForgeLogin>,
    ) -> Result<()> {
        let (store, machines, _) = self.host_reads(project, true).await?;
        let logins = match logins.is_empty() {
            true => vec![my_login(&store, &machines).await?],
            false => logins,
        };

        Ok(assign(&store, &machines, number, &logins).await?)
    }

    /// Whether `record` names this mesh: a teammate may have removed it.
    fn on_team(&self, record: &TeamRecord) -> bool {
        record
            .members
            .iter()
            .any(|member| member.mesh == self.mesh.id())
    }

    /// The forge repository of `project`.
    fn team_repo(&self, project: &str) -> Result<TeamRepo> {
        let stored = find_project(&self.mesh, project).ok_or(FleetError::NotFound)?;

        stored
            .record
            .url
            .parse()
            .map_err(|_| FleetError::refused(format!("{project} is not on a forge")))
    }

    /// Changes `project`'s team record with `change`, and the contacts with it.
    async fn change_team(&self, project: &str, change: impl FnOnce(&mut TeamRecord)) -> Result<()> {
        let repo = self.team_repo(project)?;
        let held = TeamWrites::lock().await;
        let mut record = self.team_record(project)?.ok_or(FleetError::NotFound)?;
        let before = record.clone();
        change(&mut record);

        if record != before {
            write(&self.mesh, Key::Team(repo), Some(&record)).await?;
            keep_contacts(&self.mesh, &held).await?;
        }

        Ok(())
    }

    /// This mesh on `project`'s team: the login its machine is signed in
    /// with, and the machines of the mesh with the project.
    async fn own_member(&self, project: &str) -> Result<Member> {
        let (store, machines, _) = self.host_reads(project, true).await?;
        let login = my_login(&store, &machines).await?;
        let stored = find_project(&self.mesh, project).ok_or(FleetError::NotFound)?;
        let devices: Vec<EndpointAddr> = stored
            .places
            .keys()
            .filter(|machine| self.mesh.is_member(**machine))
            .map(|machine| EndpointAddr::new(*machine))
            .collect();

        if devices.is_empty() {
            return Err(FleetError::refused(format!(
                "No machine of this mesh has {project}"
            )));
        }

        Ok(Member {
            login,
            mesh: self.mesh.id(),
            devices,
            joined_at: SystemTime::now(),
            admitted_by: None,
            proof: None,
        })
    }

    /// The nearest online machine with a checkout of `project`, this one first.
    fn checkout_machine(&self, project: &str) -> Result<EndpointId> {
        let stored = find_project(&self.mesh, project).ok_or(FleetError::NotFound)?;
        let online: Vec<EndpointId> = self
            .mesh
            .devices()
            .into_iter()
            .filter(|status| status.online || status.device.id == self.local)
            .map(|status| status.device.id)
            .collect();
        let mut machines: Vec<EndpointId> = stored
            .places
            .into_keys()
            .filter(|machine| online.contains(machine))
            .collect();
        nearest_first(&self.mesh, &mut machines, |machine| *machine);

        machines
            .first()
            .copied()
            .ok_or_else(|| FleetError::refused(format!("No machine with {project} is online")))
    }

    /// Sends `args` to this desktop's own node, or a phone's nearest online
    /// one: either sends what only a team's gateway does on to it.
    async fn team_app(&self, args: &TeamAppArgs, limit: Duration) -> Result<String> {
        let mut nodes: Vec<EndpointId> = self
            .mesh
            .code_hosts()
            .into_iter()
            .filter(|status| status.online || status.device.id == self.local)
            .map(|status| status.device.id)
            .collect();
        nearest_first(&self.mesh, &mut nodes, |node| *node);
        let node = nodes
            .first()
            .copied()
            .ok_or_else(|| FleetError::refused("No machine of the mesh is online"))?;

        self.call(node, Tool::TeamApp, args, limit).await
    }

    /// Calls `tool` with `args` on `machine`: the text it answers.
    async fn call(
        &self,
        machine: EndpointId,
        tool: Tool,
        args: &impl Serialize,
        limit: Duration,
    ) -> Result<String> {
        let request = Request {
            session: SessionId::default(),
            tool,
            args: serde_json::to_string(args).map_err(anyhow::Error::from)?,
            approved: Approved::No,
        };
        let client = self.machines.client(machine).await?;
        let reply = client.call_within(limit, tools::PROTOCOL, &request).await?;

        match reply {
            Reply::Done {
                text,
                failed: false,
            } => Ok(text),
            Reply::Done { text, .. } => Err(FleetError::Refused(text)),
            _ => Err(FleetError::refused("The machine answered something else")),
        }
    }
}

fn removed() -> FleetError {
    FleetError::refused("A teammate removed this mesh from the team")
}

#[cfg(test)]
#[path = "tests/team.rs"]
mod tests;
