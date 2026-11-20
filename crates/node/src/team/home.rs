//! This mesh as the team link sees it: its teams, contacts and the projects
//! they are about, kept current from the mesh's state. Rosters and invites
//! the link takes go to a desktop, since only desktops write the state.

use super::link::{TeamForge, TeamLink};
use crate::{
    agents::mcp::to_desktop,
    machine::ci::forge::{command, tail},
};
use beep_mesh::{DeviceKind, Key, Mesh};
use beep_projects::{
    commands::{OpenIssue, parse_open_issues},
    model::{IssueHost, RepoName},
    team_forge::{
        ForgeCall, collaborator_answer, collaborator_call, open_issues_call, parse_user_login,
        user_call,
    },
};
use beep_proto::{
    SessionId, lenient,
    node::{
        mesh_state::{Place, ProjectRecord, StateValue},
        tools::{Reply, Request, TeamInviteArgs, TeamRosterArgs, Tool},
    },
    team::{ContactRecord, ForgeLogin, TeamRecord, TeamRepo},
};
use futures::future::BoxFuture;
use iroh::{EndpointAddr, EndpointId};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::{Output, Stdio},
    time::{Duration, SystemTime},
};
use tokio::{
    select,
    sync::broadcast::error::RecvError,
    time::{MissedTickBehavior, interval, timeout},
};

/// How often what waits for a desktop is offered again.
const RETRY: Duration = Duration::from_secs(60);
/// How long a forge call may take.
const FORGE_LIMIT: Duration = Duration::from_secs(30);
/// How long a contact's invite waits for an answer, and how many from one
/// contact wait at once.
const INVITE_AGE: Duration = Duration::from_secs(24 * 60 * 60);
const INVITES_PER_CONTACT: usize = 8;

#[derive(Debug, Clone, Default)]
pub(crate) struct Home {
    /// This mesh's id, once the device is in one.
    pub(crate) mesh: Option<EndpointId>,
    /// The mesh's projects by repository.
    pub(crate) projects: BTreeMap<TeamRepo, Project>,
    /// Contacts' devices, with the contact's mesh.
    pub(crate) contacts: BTreeMap<EndpointId, EndpointId>,
}

#[derive(Debug, Clone)]
pub(crate) struct Project {
    /// The mesh's name for it, as its agents' sessions are listed under.
    pub(crate) name: String,
    /// This machine's checkout, when it has one.
    pub(crate) checkout: Option<Checkout>,
    /// The mesh's machines with a checkout: the devices teammates dial.
    pub(crate) devices: Vec<EndpointAddr>,
}

#[derive(Debug, Clone)]
pub(crate) struct Checkout {
    pub(crate) folder: PathBuf,
    /// The remote proofs are pushed to and read from.
    pub(crate) remote: String,
}

/// Contacts' invites waiting for a desktop: one per contact and repository,
/// a few per contact, each for a day.
#[derive(Debug, Clone, Default)]
pub(crate) struct Invites(Vec<(TeamInviteArgs, SystemTime)>);

impl Invites {
    /// Keeps `invite`, taken `now`, over an older one to the same team.
    pub(crate) fn keep(&mut self, invite: TeamInviteArgs, now: SystemTime) -> Result<(), String> {
        self.expire(now);
        self.0
            .retain(|(kept, _)| (kept.from, &kept.repo) != (invite.from, &invite.repo));
        let waiting = self.0.iter().filter(|(kept, _)| kept.from == invite.from);

        if waiting.count() >= INVITES_PER_CONTACT {
            return Err("Too many invites from you wait for an answer".to_owned());
        }

        self.0.push((invite, now));
        Ok(())
    }

    /// The invites still waiting `now`.
    pub(crate) fn current(&mut self, now: SystemTime) -> Vec<TeamInviteArgs> {
        self.expire(now);
        self.0.iter().map(|(invite, _)| invite.clone()).collect()
    }

    pub(crate) fn remove(&mut self, invite: &TeamInviteArgs) {
        self.0.retain(|(kept, _)| kept != invite);
    }

    fn expire(&mut self, now: SystemTime) {
        self.0
            .retain(|(_, at)| now.duration_since(*at).map_or(true, |age| age < INVITE_AGE));
    }
}

impl Home {
    pub(crate) fn checkout(&self, repo: &TeamRepo) -> Option<Checkout> {
        self.projects.get(repo)?.checkout.clone()
    }

    /// The repository of the project whose checkout here holds `folder`.
    pub(crate) fn team_of(&self, folder: &Path) -> Option<TeamRepo> {
        let held = self.projects.iter().filter_map(|(repo, project)| {
            let checkout = &project.checkout.as_ref()?.folder;
            folder
                .starts_with(checkout)
                .then(|| (checkout.as_os_str().len(), repo))
        });

        held.max().map(|(_, repo)| repo.clone())
    }

    pub(crate) fn devices(&self, repo: &TeamRepo) -> Vec<EndpointAddr> {
        self.projects
            .get(repo)
            .map(|project| project.devices.clone())
            .unwrap_or_default()
    }
}

/// Keeps `link` current with `mesh` and hands what waits to a desktop, until
/// the mesh is gone.
pub(crate) async fn follow(mesh: Mesh, link: TeamLink) {
    let mut changes = mesh.changes();
    let mut retry = interval(RETRY);
    retry.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut handed = Vec::new();
    refresh(&mesh, &link);

    loop {
        select! {
            change = changes.recv() => match change {
                Ok(Key::Team(_) | Key::Contact(_) | Key::Project(_)) | Err(RecvError::Lagged(_)) => {
                    refresh(&mesh, &link);
                }
                Ok(_) => continue,
                Err(RecvError::Closed) => return,
            },
            () = link.waiting() => {}
            _ = retry.tick() => {}
        }

        hand(&mesh, &link, &mut handed).await;
    }
}

fn refresh(mesh: &Mesh, link: &TeamLink) {
    let records = mesh
        .scan("team/")
        .into_iter()
        .filter_map(|(key, bytes)| match key {
            Key::Team(repo) => Some((repo, lenient::from_slice::<TeamRecord>(&bytes).ok()?)),
            _ => None,
        })
        .collect();
    let home = read(mesh);

    link.with(|teams| {
        teams.rosters.refresh(records);
        teams.home = home;
    });
}

/// Whether `folder` is in a checkout here of a project with a team.
pub(crate) fn in_team(mesh: &Mesh, folder: &Path) -> bool {
    read(mesh)
        .team_of(folder)
        .is_some_and(|repo| mesh.get(&Key::Team(repo)).is_some())
}

pub(super) fn read(mesh: &Mesh) -> Home {
    let me = mesh.me();
    let mut records = BTreeMap::new();
    let mut places: BTreeMap<String, Vec<(EndpointId, Place)>> = BTreeMap::new();

    for (key, bytes) in mesh.scan("project/") {
        let Key::Project(name) = key else { continue };

        if let Some((project, machine)) = name.rsplit_once('/')
            && let Ok(machine) = machine.parse::<EndpointId>()
            && let Ok(place) = Place::from_bytes(&bytes)
        {
            let places = places.entry(project.to_owned()).or_default();
            places.push((machine, place));
        } else if let Ok(record) = ProjectRecord::from_bytes(&bytes) {
            records.insert(name, record);
        }
    }

    let projects = records
        .into_iter()
        .filter_map(|(name, record)| {
            let repo = record.url.parse::<TeamRepo>().ok()?;
            let places = places.remove(&name).unwrap_or_default();
            let checkout = places
                .iter()
                .find(|(machine, _)| *machine == me)
                .map(|(_, place)| {
                    // A fork is `origin`; the team's repository is the project's own.
                    let remote = match place.fork {
                        Some(_) => record.url.clone(),
                        None => "origin".to_owned(),
                    };
                    Checkout {
                        folder: PathBuf::from(&place.folder),
                        remote,
                    }
                });
            let devices = places
                .iter()
                .filter(|(machine, _)| mesh.is_member(*machine))
                .map(|(machine, _)| EndpointAddr::new(*machine))
                .collect();
            let project = Project {
                name,
                checkout,
                devices,
            };
            Some((repo, project))
        })
        .collect();
    let contacts = mesh
        .scan("contact/")
        .into_iter()
        .filter_map(|(_, bytes)| lenient::from_slice::<ContactRecord>(&bytes).ok())
        .flat_map(|contact| {
            let mesh = contact.mesh;
            contact
                .devices
                .into_iter()
                .map(move |device| (device.id, mesh))
        })
        .collect();

    Home {
        mesh: Some(mesh.id()),
        projects,
        contacts,
    }
}

/// Hands the rosters and invites that wait to a desktop. A desktop writes
/// its own rosters; a roster already handed is not sent again.
async fn hand(mesh: &Mesh, link: &TeamLink, handed: &mut Vec<TeamRecord>) {
    let (records, invites) = link.with(|teams| {
        let invites = teams.invites.current(SystemTime::now());
        (teams.rosters.pending(), invites)
    });
    let desktop_here = mesh
        .devices()
        .iter()
        .any(|status| status.device.id == mesh.me() && status.device.kind == DeviceKind::Desktop);
    handed.retain(|record| records.contains(record));
    let unhanded: Vec<_> = records
        .into_iter()
        .filter(|record| !handed.contains(record))
        .collect();

    for record in unhanded {
        let written = match desktop_here {
            true => write(mesh, &record).await,
            false => {
                call(
                    mesh,
                    Tool::TeamRoster,
                    &TeamRosterArgs {
                        record: record.clone(),
                    },
                )
                .await
            }
        };

        match written {
            Ok(()) => handed.push(record),
            Err(e) => log::info!("the roster of {} waits for a desktop: {e}", record.repo),
        }
    }

    // A desktop's own node keeps them for the desktop.
    if desktop_here {
        return;
    }

    for invite in invites {
        match call(mesh, Tool::TeamInvite, &invite).await {
            Ok(()) => link.with(|teams| teams.invites.remove(&invite)),
            Err(e) => log::info!("an invite to {} waits for a desktop: {e}", invite.repo),
        }
    }
}

async fn write(mesh: &Mesh, record: &TeamRecord) -> Result<(), String> {
    let bytes = rmp_serde::to_vec(record).map_err(|e| e.to_string())?;

    mesh.set(Key::Team(record.repo.clone()), Some(bytes))
        .await
        .map_err(|e| e.to_string())
}

/// Calls `tool` with `args` on a desktop of the mesh.
async fn call(mesh: &Mesh, tool: Tool, args: &impl Serialize) -> Result<(), String> {
    let request = Request {
        session: SessionId::default(),
        tool,
        args: serde_json::to_string(args).map_err(|e| e.to_string())?,
        approved: Default::default(),
    };

    match to_desktop(mesh, &request).await {
        Reply::Done { failed: false, .. } => Ok(()),
        Reply::Done { text, .. } => Err(text),
        _ => Err("the desktop answered something else".to_owned()),
    }
}

/// The forge through this machine's `gh` and `teacli`.
pub(crate) struct ForgeCli;

impl TeamForge for ForgeCli {
    fn collaborator<'a>(
        &'a self,
        repo: &'a TeamRepo,
        login: &'a ForgeLogin,
    ) -> BoxFuture<'a, Result<bool, String>> {
        Box::pin(async move {
            let (host, name) = forge_of(repo)?;
            let out = run(&collaborator_call(&host, &name, login)).await?;
            let (stdout, stderr) = (
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr),
            );

            collaborator_answer(out.status.success(), &stdout, &stderr).map_err(|e| e.to_string())
        })
    }

    fn login<'a>(&'a self, repo: &'a TeamRepo) -> BoxFuture<'a, Result<ForgeLogin, String>> {
        Box::pin(async move {
            let (host, _) = forge_of(repo)?;
            let out = run(&user_call(&host)).await?;

            if !out.status.success() {
                let said = String::from_utf8_lossy(&out.stderr);
                return Err(format!(
                    "Could not read your account on {}: {}",
                    repo.host,
                    tail(&said)
                ));
            }

            parse_user_login(&String::from_utf8_lossy(&out.stdout)).map_err(|e| e.to_string())
        })
    }

    fn open_issues<'a>(
        &'a self,
        repo: &'a TeamRepo,
    ) -> BoxFuture<'a, Result<Vec<OpenIssue>, String>> {
        Box::pin(async move {
            let (host, name) = forge_of(repo)?;
            let out = run(&open_issues_call(&host, &name)).await?;

            if !out.status.success() {
                let said = String::from_utf8_lossy(&out.stderr);
                return Err(format!(
                    "Could not list the issues of {repo}: {}",
                    tail(&said)
                ));
            }

            parse_open_issues(&String::from_utf8_lossy(&out.stdout)).map_err(|e| format!("{e:#}"))
        })
    }
}

fn forge_of(repo: &TeamRepo) -> Result<(IssueHost, RepoName), String> {
    let host = IssueHost::of_hostname(&repo.host)
        .ok_or_else(|| format!("{} is not a forge Beep can ask", repo.host))?;
    let name = repo
        .repo
        .parse()
        .map_err(|_| format!("{repo} is not an owner/repository"))?;

    Ok((host, name))
}

async fn run(call: &ForgeCall) -> Result<Output, String> {
    let mut command = command(call.program, &call.args);
    command.stdout(Stdio::piped()).stderr(Stdio::piped());

    timeout(FORGE_LIMIT, command.output())
        .await
        .map_err(|_| format!("{} took too long", call.program))?
        .map_err(|e| format!("Could not start {}: {e}", call.program))
}
