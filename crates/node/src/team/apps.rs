//! What this mesh's devices ask of the team link besides the agents' tools.
//! A node hands a desktop the rosters it changed and the contacts' invites it
//! took (`Tool::TeamRoster`, `Tool::TeamInvite`). The apps read a board's
//! cards, dismiss them, tell teammates of a removal or a leave, invite a
//! contact and read the invites kept (`Tool::TeamApp`). What only a team's
//! gateway knows, or may send, goes on to that gateway once.

use super::{
    cards::Teammates,
    link::{TeamError, TeamLink},
};
use crate::{agents::mcp::over_the_mesh, link::peer_tools::Caller, run::tasks::Table};
use beep_mesh::{Mesh, take_roster};
use beep_proto::{
    node::tools::{Reply, Request, TeamAppArgs, TeamCards, TeamInviteArgs, TeamRosterArgs, Tool},
    team::{TeamMsg, TeamRepo},
};
use iroh::EndpointAddr;
use serde::{Serialize, de::DeserializeOwned};
use std::time::{Duration, SystemTime};
use tokio::time::timeout;

/// How long an invite may take to reach a contact, over all their devices.
const INVITE_LIMIT: Duration = Duration::from_secs(60);

/// A result for the caller, or why the call did not work.
type Outcome = Result<String, String>;

/// Answers `Tool::TeamRoster`, `Tool::TeamInvite` and `Tool::TeamApp`.
pub(crate) async fn answer(caller: &Caller, request: &Request) -> Reply {
    let mesh = caller.agents.as_ref().and_then(|agents| agents.mesh());
    let Some(link) = &caller.team else {
        return reply(Err("This machine does not act for a team.".to_owned()));
    };

    match request.tool {
        Tool::TeamRoster | Tool::TeamInvite if !caller.desktop_here => {
            reply(Err("Only a desktop records teams.".to_owned()))
        }
        Tool::TeamRoster => reply(roster(mesh, request).await),
        Tool::TeamInvite => reply(keep_invite(link, request)),
        _ => {
            let from_peer = matches!(caller.table, Table::Peer(_));
            app(link, mesh, from_peer, request).await
        }
    }
}

/// Writes a roster a node changed.
async fn roster(mesh: Option<Mesh>, request: &Request) -> Outcome {
    let mesh = mesh.ok_or("This machine is in no mesh.")?;
    let TeamRosterArgs { record } = parse(&request.args)?;
    let repo = record.repo.clone();
    take_roster(&mesh, record)
        .await
        .map_err(|e| e.to_string())?;

    Ok(format!("Recorded the team of {repo}."))
}

/// Keeps a contact's invite a node took, for this desktop to show.
fn keep_invite(link: &TeamLink, request: &Request) -> Outcome {
    let invite: TeamInviteArgs = parse(&request.args)?;
    link.with(|teams| teams.invites.keep(invite, SystemTime::now()))?;

    Ok("Kept the invite.".to_owned())
}

/// Answers an app's call here, or sends it on to the team's gateway when an
/// app of this mesh made it here.
async fn app(link: &TeamLink, mesh: Option<Mesh>, from_peer: bool, request: &Request) -> Reply {
    let args: TeamAppArgs = match parse(&request.args) {
        Ok(args) => args,
        Err(e) => return reply(Err(e)),
    };
    let repo = match &args {
        TeamAppArgs::Cards { repo }
        | TeamAppArgs::Dismiss { repo, .. }
        | TeamAppArgs::Remove { repo, .. }
        | TeamAppArgs::Leave { repo }
        | TeamAppArgs::Invite { via: repo, .. } => Some(repo),
        TeamAppArgs::Invites | TeamAppArgs::DropInvite { .. } => None,
    };

    if let Some(repo) = repo
        && !from_peer
    {
        let gateway = link.with(|teams| teams.gateways.get(repo).copied());

        match (gateway, &mesh) {
            (Some(gateway), Some(mesh)) if gateway != mesh.me() => {
                return over_the_mesh(mesh, gateway, request)
                    .await
                    .unwrap_or_else(|e| {
                        reply(Err(format!(
                            "The machine that acts for the team did not answer: {e:#}"
                        )))
                    });
            }
            (Some(_), Some(_)) => {}
            _ => {
                return reply(Err(
                    "No machine of this mesh acts for the team yet; try again in a minute."
                        .to_owned(),
                ));
            }
        }
    }

    reply(here(link, args).await)
}

/// The answer of this device to an app's call.
async fn here(link: &TeamLink, args: TeamAppArgs) -> Outcome {
    let now = SystemTime::now();

    match args {
        TeamAppArgs::Cards { repo } => json(&link.with(|teams| {
            TeamCards {
                needs_you: teams.asks.cards(&repo, now),
                teammates: teams
                    .teammates
                    .get(&repo)
                    .map(Teammates::cards)
                    .unwrap_or_default(),
            }
        })),
        TeamAppArgs::Dismiss { repo, card } => {
            link.dismiss_card(&repo, &card);
            Ok("Dismissed.".to_owned())
        }
        TeamAppArgs::Remove { repo, login } => {
            let remove = TeamMsg::Remove {
                repo: repo.clone(),
                login,
            };
            tell(link, &repo, remove)
        }
        TeamAppArgs::Leave { repo } => {
            let login = link
                .with(|teams| {
                    let own = teams.home.mesh;
                    let mut members = teams.rosters.get(&repo)?.members.iter();
                    members
                        .find(|member| Some(member.mesh) == own)
                        .map(|member| member.login.clone())
                })
                .ok_or_else(|| not_on(&repo))?;
            let leave = TeamMsg::Leave {
                repo: repo.clone(),
                login,
            };
            tell(link, &repo, leave)
        }
        TeamAppArgs::Invite {
            repo,
            ticket,
            devices,
            ..
        } => invite(link, &devices, TeamMsg::Invite { repo, ticket }).await,
        TeamAppArgs::Invites => json(&link.with(|teams| teams.invites.current(now))),
        TeamAppArgs::DropInvite { invite } => {
            link.with(|teams| teams.invites.remove(&invite));
            Ok("Dropped the invite.".to_owned())
        }
    }
}

/// Sends `msg` to every member of `repo`'s team from another mesh, in the
/// background.
fn tell(link: &TeamLink, repo: &TeamRepo, msg: TeamMsg) -> Outcome {
    let others = link
        .with(|teams| {
            let own = teams.home.mesh;
            let members = teams.rosters.get(repo)?.members.iter();
            let others = members.filter(|member| Some(member.mesh) != own);
            Some(others.cloned().collect())
        })
        .ok_or_else(|| not_on(repo))?;
    link.tell(others, msg);

    Ok("Telling the team.".to_owned())
}

/// Sends a contact's first device that answers an invite, within
/// `INVITE_LIMIT`.
async fn invite(link: &TeamLink, devices: &[EndpointAddr], msg: TeamMsg) -> Outcome {
    let tries = async {
        for device in devices {
            match link.offer(device, &msg).await {
                Ok(None) => return Ok("Sent the invite.".to_owned()),
                Ok(Some(reason)) => return Err(reason),
                Err(e) => log::debug!("an invite did not reach {}: {e}", device.id.fmt_short()),
            }
        }

        Err(TeamError::Unreachable.to_string())
    };

    timeout(INVITE_LIMIT, tries)
        .await
        .unwrap_or_else(|_| Err("The contact did not answer in time".to_owned()))
}

fn not_on(repo: &TeamRepo) -> String {
    format!("This mesh is not on the team of {repo}")
}

fn parse<T: DeserializeOwned>(args: &str) -> Result<T, String> {
    serde_json::from_str(args).map_err(|e| format!("The arguments do not fit: {e}"))
}

fn json(value: &impl Serialize) -> Outcome {
    serde_json::to_string(value).map_err(|e| e.to_string())
}

fn reply(outcome: Outcome) -> Reply {
    match outcome {
        Ok(text) => Reply::Done {
            text,
            failed: false,
        },
        Err(text) => Reply::Done { text, failed: true },
    }
}
