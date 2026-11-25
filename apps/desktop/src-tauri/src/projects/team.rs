//! A project's team: its record, invite codes, joining and leaving, the
//! board's Needs you and teammates' cards, contacts and their invites, and
//! assigning issues.

use crate::{
    beep::epoch_ms,
    error::{DesktopError, Result},
    fleet::{
        Fleet, Machines,
        projects::{FleetState, desk, named},
    },
    machines::list::MachineLists,
};
use beep_fleet::{Desk, ImportJob};
use beep_proto::{
    FoundPlace, FoundProject,
    node::tools::{TeamCards, TeamInviteArgs},
    team::{Card, ContactRecord, ForgeLogin, TeamRecord, TeamRepo},
};
use iroh::EndpointId;
use serde::Serialize;
use std::str::FromStr;
use tauri::State;
use uuid::Uuid;

/// A project's team as the Team page shows it.
#[derive(Debug, PartialEq, Serialize)]
pub struct TeamView {
    repo: String,
    /// This mesh's login on the team; `None` while it is not on it.
    me: Option<String>,
    members: Vec<MemberView>,
    pickup: bool,
    pickup_limit: Option<u32>,
    asks: bool,
    /// The logins removed or gone, who may be let back in.
    removed: Vec<String>,
}

#[derive(Debug, PartialEq, Serialize)]
struct MemberView {
    login: String,
    mesh: String,
    /// In milliseconds since the Unix epoch.
    joined_at: u64,
}

impl TeamView {
    fn of(record: TeamRecord, own: EndpointId) -> Self {
        let me = record.members.iter().find(|member| member.mesh == own);

        Self {
            repo: record.repo.to_string(),
            me: me.map(|member| member.login.to_string()),
            members: record
                .members
                .iter()
                .map(|member| MemberView {
                    login: member.login.to_string(),
                    mesh: member.mesh.to_string(),
                    joined_at: epoch_ms(member.joined_at),
                })
                .collect(),
            pickup: record.pickup,
            pickup_limit: record.pickup_limit,
            asks: record.asks,
            removed: record
                .removed
                .into_iter()
                .map(|gone| gone.login.to_string())
                .collect(),
        }
    }
}

/// A card on a team's board.
#[derive(Debug, PartialEq, Serialize)]
struct CardView {
    id: String,
    title: String,
    /// `needs_you`, `working`, `ready_to_review`, `done_today` or `queued`.
    lane: &'static str,
    login: String,
    machine: String,
    issue: Option<u64>,
    pr: Option<u64>,
    /// In milliseconds since the Unix epoch.
    updated_at: u64,
}

impl From<Card> for CardView {
    fn from(card: Card) -> Self {
        Self {
            id: card.id,
            title: card.title,
            lane: card.lane.into(),
            login: card.login.to_string(),
            machine: card.machine,
            issue: card.issue,
            pr: card.pr,
            updated_at: epoch_ms(card.updated_at),
        }
    }
}

/// The cards a project's board adds for its team.
#[derive(Debug, PartialEq, Serialize)]
pub struct TeamCardsView {
    /// Asks for the user, and automatic sessions that ended without a pull request.
    needs_you: Vec<CardView>,
    teammates: Vec<CardView>,
    /// The user's own issues the pickup limit holds back.
    own_queued: Vec<CardView>,
    /// The teammates whose board the mesh follows now, cards or none.
    online: Vec<String>,
}

impl From<TeamCards> for TeamCardsView {
    fn from(cards: TeamCards) -> Self {
        Self {
            needs_you: cards.needs_you.into_iter().map(CardView::from).collect(),
            teammates: cards.teammates.into_iter().map(CardView::from).collect(),
            own_queued: cards.own_queued.into_iter().map(CardView::from).collect(),
            online: cards.online.iter().map(ToString::to_string).collect(),
        }
    }
}

/// A person of another mesh on the user's teams.
#[derive(Debug, Serialize)]
pub struct ContactView {
    mesh: String,
    logins: Vec<ContactLogin>,
}

#[derive(Debug, Serialize)]
struct ContactLogin {
    repo: String,
    login: String,
}

impl From<ContactRecord> for ContactView {
    fn from(contact: ContactRecord) -> Self {
        Self {
            mesh: contact.mesh.to_string(),
            logins: contact
                .logins
                .into_iter()
                .map(|(repo, login)| ContactLogin {
                    repo: repo.to_string(),
                    login: login.to_string(),
                })
                .collect(),
        }
    }
}

/// A contact's invite to their team, waiting for Accept.
#[derive(Debug, Serialize)]
pub struct InviteView {
    /// The contact's mesh.
    from: String,
    /// A login the contact is known by, when they still share a team.
    login: Option<String>,
    repo: String,
    /// The `beepteam1` code Accept joins with.
    ticket: String,
}

/// `name`'s team as the Team page shows it.
pub(crate) fn team_view(desk: &Desk, name: &str) -> beep_fleet::Result<Option<TeamView>> {
    let own = desk.mesh_id();

    Ok(desk
        .team_record(name)?
        .map(|record| TeamView::of(record, own)))
}

/// A new invite code for the project's team, which starts the team the first time.
#[tauri::command]
pub async fn team_invite(fleet: FleetState<'_>, project_id: Uuid) -> Result<String> {
    let (desk, name) = named(&fleet, project_id).await?;

    Ok(desk.team_invite(&name).await?)
}

/// What a `beepteam1` code invites to, before joining.
#[derive(Debug, Serialize)]
pub struct TicketView {
    /// Lower case, with its port when it has one.
    host: String,
    /// `org/name`.
    repo: String,
    /// The project that has the repository here; `None` until it is imported.
    project: Option<String>,
}

#[tauri::command]
pub async fn team_ticket(fleet: FleetState<'_>, ticket: String) -> Result<TicketView> {
    let desk = desk(&fleet).await?;
    let (repo, project) = desk.team_ticket(&ticket)?;

    Ok(TicketView {
        host: repo.host,
        repo: repo.repo,
        project,
    })
}

/// The project ready for a team join: either already imported or added now.
#[derive(Debug, Serialize)]
pub struct PreparedProject {
    pub project: String,
    pub machine_name: Option<String>,
    pub folder: Option<String>,
    pub added: bool,
    pub cloned: bool,
}

#[tauri::command]
pub async fn team_prepare(
    fleet: Machines<'_>,
    session: FleetState<'_>,
    held: State<'_, MachineLists>,
    ticket: String,
) -> Result<PreparedProject> {
    let desk = desk(&session).await?;
    let (repo, existing) = desk.team_ticket(&ticket)?;

    ensure_team_project(&fleet, &desk, &held, &repo, existing, None, None).await
}

/// Makes sure `repo` is imported as a project before joining its team: uses
/// `existing` when already imported, links a matching found checkout on an
/// online machine when one exists, or clones `https://{repo}`.
pub(super) async fn ensure_team_project(
    fleet: &Fleet,
    desk: &Desk,
    held: &MachineLists,
    repo: &TeamRepo,
    existing: Option<String>,
    machine_id: Option<&str>,
    folder: Option<&str>,
) -> Result<PreparedProject> {
    if let Some(project) = existing {
        return Ok(PreparedProject {
            project,
            machine_name: None,
            folder: None,
            added: false,
            cloned: false,
        });
    }

    let online: Vec<String> = fleet
        .online()
        .into_iter()
        .map(|machine| machine.to_string())
        .collect();
    let found = desk.group_found(held.found.last());

    if let Some((remote, place)) = find_matching_place(&found, repo, &online, machine_id) {
        let imported = desk
            .import(ImportJob {
                repo: &remote,
                machine_id: Some(&place.machine_id),
                folder: Some(&place.folder),
                parent: None,
                worktrees: &[],
            })
            .await?;

        return Ok(PreparedProject {
            project: imported.project,
            machine_name: Some(imported.machine_name),
            folder: Some(imported.folder),
            added: true,
            cloned: imported.cloned,
        });
    }

    let url = format!("https://{repo}");
    let imported = desk
        .import(ImportJob {
            repo: &url,
            machine_id,
            folder: None,
            parent: folder,
            worktrees: &[],
        })
        .await?;

    Ok(PreparedProject {
        project: imported.project,
        machine_name: Some(imported.machine_name),
        folder: Some(imported.folder),
        added: true,
        cloned: imported.cloned,
    })
}

/// The first online checkout in `found` whose remote matches `repo` (and
/// `machine_id`, when given).
pub(super) fn find_matching_place(
    found: &[FoundProject],
    repo: &TeamRepo,
    online: &[String],
    machine_id: Option<&str>,
) -> Option<(String, FoundPlace)> {
    let project = found
        .iter()
        .find(|project| project.remote.parse::<TeamRepo>().as_ref() == Ok(repo))?;
    let place = project.places.iter().find(|place| {
        online.contains(&place.machine_id)
            && machine_id.is_none_or(|wanted| place.machine_id == wanted)
    })?;

    Some((project.remote.clone(), place.clone()))
}

#[tauri::command]
pub async fn team_join(fleet: FleetState<'_>, ticket: String) -> Result<TeamView> {
    let desk = desk(&fleet).await?;
    let record = desk.team_join(&ticket).await?;

    Ok(TeamView::of(record, desk.mesh_id()))
}

#[tauri::command]
pub async fn team_remove(fleet: FleetState<'_>, project_id: Uuid, login: String) -> Result<()> {
    let (desk, name) = named(&fleet, project_id).await?;

    Ok(desk.team_remove(&name, &parse(&login)?).await?)
}

#[tauri::command]
pub async fn team_leave(fleet: FleetState<'_>, project_id: Uuid) -> Result<()> {
    let (desk, name) = named(&fleet, project_id).await?;

    Ok(desk.team_leave(&name).await?)
}

/// Lets a login removed or gone join the team again.
#[tauri::command]
pub async fn team_readmit(fleet: FleetState<'_>, project_id: Uuid, login: String) -> Result<()> {
    let (desk, name) = named(&fleet, project_id).await?;

    Ok(desk.team_readmit(&name, &parse(&login)?).await?)
}

#[tauri::command]
pub async fn team_settings(
    fleet: FleetState<'_>,
    project_id: Uuid,
    pickup: bool,
    pickup_limit: Option<u32>,
    asks: bool,
) -> Result<()> {
    let (desk, name) = named(&fleet, project_id).await?;

    Ok(desk
        .team_settings(&name, pickup, pickup_limit, asks)
        .await?)
}

#[tauri::command]
pub async fn team_invite_contact(
    fleet: FleetState<'_>,
    project_id: Uuid,
    contact: String,
) -> Result<()> {
    let (desk, name) = named(&fleet, project_id).await?;

    Ok(desk.team_invite_contact(&name, parse(&contact)?).await?)
}

#[tauri::command]
pub async fn contacts(fleet: FleetState<'_>) -> Result<Vec<ContactView>> {
    let desk = desk(&fleet).await?;

    Ok(desk.contacts().into_iter().map(ContactView::from).collect())
}

/// Removes a contact from every team shared with them, and forgets them.
#[tauri::command]
pub async fn contact_remove(fleet: FleetState<'_>, contact: String) -> Result<()> {
    let desk = desk(&fleet).await?;

    Ok(desk.remove_contact(parse(&contact)?).await?)
}

/// Assigns `logins` to issue `number`; none assigns the user.
#[tauri::command]
pub async fn issue_assign(
    fleet: FleetState<'_>,
    project_id: Uuid,
    number: u64,
    logins: Vec<String>,
) -> Result<()> {
    let (desk, name) = named(&fleet, project_id).await?;
    let logins = logins
        .iter()
        .map(|login| parse::<ForgeLogin>(login))
        .collect::<Result<Vec<_>>>()?;

    Ok(desk.assign_issue(&name, number, logins).await?)
}

#[tauri::command]
pub async fn team_cards(fleet: FleetState<'_>, project_id: Uuid) -> Result<TeamCardsView> {
    let (desk, name) = named(&fleet, project_id).await?;

    Ok(desk.team_cards(&name).await?.into())
}

/// Drops a Needs you card of the project's team.
#[tauri::command]
pub async fn team_dismiss(fleet: FleetState<'_>, project_id: Uuid, card: String) -> Result<()> {
    let (desk, name) = named(&fleet, project_id).await?;

    Ok(desk.team_dismiss(&name, &card).await?)
}

/// The invites as the window shows them, with a login the contact is known by.
pub(crate) fn invite_views(desk: &Desk, invites: Vec<TeamInviteArgs>) -> Vec<InviteView> {
    let contacts = desk.contacts();

    invites
        .into_iter()
        .map(|invite| {
            let contact = contacts.iter().find(|contact| contact.mesh == invite.from);
            InviteView {
                from: invite.from.to_string(),
                login: contact
                    .and_then(|contact| contact.logins.first())
                    .map(|(_, login)| login.to_string()),
                repo: invite.repo.to_string(),
                ticket: invite.ticket,
            }
        })
        .collect()
}

/// Drops a contact's invite, once accepted or declined.
#[tauri::command]
pub async fn team_invite_drop(
    fleet: FleetState<'_>,
    from: String,
    repo: String,
    ticket: String,
) -> Result<()> {
    let desk = desk(&fleet).await?;
    let invite = TeamInviteArgs {
        from: parse(&from)?,
        repo: parse(&repo)?,
        ticket,
    };

    Ok(desk.team_invite_drop(invite).await?)
}

/// `text` read as a `T`, or the request refused with why.
fn parse<T: FromStr<Err: std::fmt::Display>>(text: &str) -> Result<T> {
    text.parse()
        .map_err(|e: T::Err| DesktopError::BadRequest(e.to_string()))
}

#[cfg(test)]
#[path = "../tests/projects_team.rs"]
mod tests;
