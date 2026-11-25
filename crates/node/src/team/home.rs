//! This mesh as the team link sees it: its teams, contacts and the projects
//! they are about, kept current from the mesh's state. Rosters and invites
//! the link takes go to a desktop, since only desktops write the state.

use super::link::{TeamForge, TeamLink};
use crate::{
    agents::mcp::to_desktop,
    machine::{
        ci::forge::{command, tail},
        spawn_env,
    },
};
use beep_mesh::{DeviceKind, Key, Mesh, TeamWrites, keep_contacts, take_roster};
use beep_projects::{
    github_owner_var,
    commands::{OpenIssue, parse_open_issues, parse_open_prs},
    model::{IssueHost, RepoName},
    team_forge::{
        ForgeCall, assigned_issues_call, assignment_events_call, parse_self_assigned, collaborator_answer, collaborator_call, open_issues_call,
        branch_pulls_call, parse_stampers, parse_user_login, permission_answer, permission_call,
        repo_call, stamp_call, stamps_call, user_call,
    },
};
use beep_proto::{
    GITHUB_ENV_VAR, SessionId, lenient,
    node::{
        mesh_state::{Place, ProjectRecord, StateValue},
        tools::{Reply, Request, TeamInviteArgs, TeamRosterArgs, Tool},
    },
    team::{ContactRecord, ForgeLogin, TeamRecord, TeamRepo},
};
use futures::future::{BoxFuture, join_all, pending};
use iroh::{EndpointAddr, EndpointId};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    mem,
    path::{Path, PathBuf},
    process::{Output, Stdio},
    time::{Duration, SystemTime},
};
use tokio::{
    join,
    process::Command,
    select,
    sync::broadcast::error::RecvError,
    task::JoinSet,
    time::{Instant, sleep, sleep_until, timeout},
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

    pub(crate) fn expire(&mut self, now: SystemTime) {
        self.0
            .retain(|(_, at)| now.duration_since(*at).map_or(true, |age| age < INVITE_AGE));
    }

    /// When the oldest invite expires.
    pub(crate) fn due(&self) -> Option<SystemTime> {
        self.0.iter().map(|(_, at)| *at + INVITE_AGE).min()
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
/// the mesh is gone. The handing runs as a task of its own, one at a time, so
/// a desktop that answers slowly never holds up a refresh; what changed while
/// it ran is handed on its next run.
pub(crate) async fn follow(mesh: Mesh, link: TeamLink) {
    let mut changes = mesh.changes();
    let mut handing: JoinSet<Handed> = JoinSet::new();
    let mut handed = Vec::new();
    let mut again = true;
    // Set while a roster or an invite is still waiting for a desktop.
    let mut retry_at: Option<Instant> = None;
    refresh(&mesh, &link);

    loop {
        if again && handing.is_empty() {
            again = false;
            handing.spawn(hand(mesh.clone(), link.clone(), mem::take(&mut handed)));
        }

        let due = retry_at;
        let retry = async {
            match due {
                Some(at) => sleep_until(at).await,
                None => pending().await,
            }
        };

        let expiry = async {
            match link.next_expiry() {
                Some(at) => sleep(at.duration_since(SystemTime::now()).unwrap_or_default()).await,
                None => pending().await,
            }
        };

        select! {
            // An invite or a card that outlives its day goes, and its topic says so.
            () = expiry => link.expire_now(),
            () = link.expiry_changed() => {}
            change = changes.recv() => match change {
                Ok(Key::Team(_) | Key::Contact(_) | Key::Project(_)) | Err(RecvError::Lagged(_)) => {
                    refresh(&mesh, &link);
                    again = true;
                }
                Ok(_) => {}
                Err(RecvError::Closed) => return,
            },
            () = link.waiting() => again = true,
            () = retry => {
                retry_at = None;
                again = true;
            }
            Some(done) = handing.join_next() => {
                // A task that panicked hands nothing over; the next run starts again.
                let done = done.unwrap_or_default();
                handed = done.handed;
                retry_at = done.waiting.then(|| Instant::now() + RETRY);
            }
        }
    }
}

/// What a run of `hand` leaves for the next.
#[derive(Default)]
struct Handed {
    /// The rosters a desktop took, so they are not sent again.
    handed: Vec<TeamRecord>,
    /// Whether something still waits for a desktop.
    waiting: bool,
}

/// Whether this device is a desktop, which writes the mesh's state.
fn desktop_here(mesh: &Mesh) -> bool {
    mesh.devices()
        .iter()
        .any(|status| status.device.id == mesh.me() && status.device.kind == DeviceKind::Desktop)
}

fn refresh(mesh: &Mesh, link: &TeamLink) {
    let records = mesh
        .scan("team/")
        .into_iter()
        .filter_map(|(key, bytes)| match key {
            Key::Team(repo) => Some((repo, lenient::from_slice::<TeamRecord>(&bytes).ok()?)),
            _ => None,
        })
        .filter(|(_, record)| on_team(record, mesh))
        .collect();
    let home = read(mesh);

    link.with(|teams| teams.rosters.refresh(records));
    link.set_home(home);
}

/// Whether `record` names this mesh: one a teammate removed it from does
/// not, and is as good as left until a desktop deletes it.
fn on_team(record: &TeamRecord, mesh: &Mesh) -> bool {
    record.members.iter().any(|member| member.mesh == mesh.id())
}

/// The repository of the team whose project has the checkout `folder` is
/// in here.
pub(crate) fn team_repo_of(mesh: &Mesh, folder: &Path) -> Option<TeamRepo> {
    read(mesh)
        .team_of(folder)
        .filter(|repo| team(mesh, repo).is_some())
}

/// Whether `repo`'s team, as the mesh holds it, picks up issues assigned to
/// this mesh's person.
pub(crate) fn picks_up(mesh: &Mesh, repo: &TeamRepo) -> bool {
    team(mesh, repo).is_some_and(|record| record.pickup)
}

/// `repo`'s team as the mesh holds it, when this mesh is on it.
fn team(mesh: &Mesh, repo: &TeamRepo) -> Option<TeamRecord> {
    let bytes = mesh.get(&Key::Team(repo.clone()))?;
    let record = lenient::from_slice::<TeamRecord>(&bytes).ok()?;

    on_team(&record, mesh).then_some(record)
}

pub(crate) fn read(mesh: &Mesh) -> Home {
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

/// Keeps the contacts in step with the teams, then hands the rosters and
/// invites that wait to a desktop. A desktop writes its own rosters; a roster
/// already in `handed` is not sent again. The calls to the desktop run
/// together.
async fn hand(mesh: Mesh, link: TeamLink, mut handed: Vec<TeamRecord>) -> Handed {
    let desktop_here = desktop_here(&mesh);

    // A desktop's node writes rosters itself, so its desktop keeps the
    // contacts in step with them here.
    if desktop_here
        && let Err(e) = keep_contacts(&mesh, &TeamWrites::lock().await).await
    {
        log::warn!("the contacts were not kept in step with the teams: {e}");
    }

    let invites = link.invites_waiting();
    let records = link.with(|teams| teams.rosters.pending());
    handed.retain(|record| records.contains(record));
    let unhanded: Vec<_> = records
        .into_iter()
        .filter(|record| !handed.contains(record))
        .collect();

    let rosters = join_all(unhanded.into_iter().map(|record| async {
        let written = match desktop_here {
            true => take_roster(&mesh, record.clone(), &TeamWrites::lock().await)
                .await
                .map(drop)
                .map_err(|e| e.to_string()),
            false => {
                call(
                    &mesh,
                    Tool::TeamRoster,
                    &TeamRosterArgs {
                        record: record.clone(),
                    },
                )
                .await
            }
        };

        (record, written)
    }));

    // A desktop's own node keeps the invites for the desktop.
    let invites = join_all(invites.into_iter().filter(|_| !desktop_here).map(|invite| async {
        let sent = call(&mesh, Tool::TeamInvite, &invite).await;

        (invite, sent)
    }));

    let (rosters, invites) = join!(rosters, invites);
    let mut waiting = false;

    for (record, written) in rosters {
        match written {
            Ok(()) => handed.push(record),
            Err(e) => {
                waiting = true;
                log::info!("the roster of {} waits for a desktop: {e}", record.repo);
            }
        }
    }

    for (invite, sent) in invites {
        match sent {
            Ok(()) => link.drop_invite(&invite),
            Err(e) => {
                waiting = true;
                log::info!("an invite to {} waits for a desktop: {e}", invite.repo);
            }
        }
    }

    Handed { handed, waiting }
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
            let out = run(&permission_call(&host, &name, login), &name).await?;
            let (stdout, stderr) = (
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr),
            );
            let answer = permission_answer(out.status.success(), &stdout, &stderr);
            let pushes = match answer.map_err(|e| e.to_string())? {
                Some(pushes) => pushes,
                None => {
                    let out = run(&collaborator_call(&host, &name, login), &name).await?;
                    let stderr = String::from_utf8_lossy(&out.stderr);

                    collaborator_answer(out.status.success(), &stderr).map_err(|e| e.to_string())?
                }
            };

            // A 404 is also what a repository this account cannot see gives, as
            // after its own access changed: it judges no one then.
            if !pushes {
                let out = run(&repo_call(&host, &name), &name).await?;

                if !out.status.success() {
                    let said = String::from_utf8_lossy(&out.stderr);
                    return Err(format!("This account cannot see {repo}: {}", tail(&said)));
                }
            }

            Ok(pushes)
        })
    }

    fn login<'a>(&'a self, repo: &'a TeamRepo) -> BoxFuture<'a, Result<ForgeLogin, String>> {
        Box::pin(async move {
            let (host, name) = forge_of(repo)?;
            let out = run(&user_call(&host), &name).await?;

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
            let out = listed(repo, "issues", &open_issues_call(&host, &name), &name).await?;

            parse_open_issues(&out).map_err(|e| format!("{e:#}"))
        })
    }

    fn assigned_issues<'a>(
        &'a self,
        repo: &'a TeamRepo,
        login: &'a ForgeLogin,
    ) -> BoxFuture<'a, Result<Vec<OpenIssue>, String>> {
        Box::pin(async move {
            let (host, name) = forge_of(repo)?;
            let call = assigned_issues_call(&host, &name, login);
            let out = listed(repo, "issues", &call, &name).await?;

            parse_open_issues(&out).map_err(|e| format!("{e:#}"))
        })
    }

    fn self_assigned<'a>(
        &'a self,
        repo: &'a TeamRepo,
        number: u64,
        login: &'a ForgeLogin,
    ) -> BoxFuture<'a, Result<bool, String>> {
        Box::pin(async move {
            let (host, name) = forge_of(repo)?;
            let call = assignment_events_call(&host, &name, number);
            let out = listed(repo, "assignments", &call, &name).await?;

            parse_self_assigned(&out, login).map_err(|e| format!("{e:#}"))
        })
    }

    fn pull_heads<'a>(
        &'a self,
        repo: &'a TeamRepo,
        branch: &'a str,
    ) -> BoxFuture<'a, Result<Vec<String>, String>> {
        Box::pin(async move {
            let (host, name) = forge_of(repo)?;
            let call = branch_pulls_call(&host, &name, branch);
            let out = listed(repo, "pull requests", &call, &name).await?;
            let pulls = parse_open_prs(&out).map_err(|e| format!("{e:#}"))?;

            Ok(pulls.into_iter().map(|pull| pull.head).collect())
        })
    }

    fn stamp<'a>(
        &'a self,
        repo: &'a TeamRepo,
        sha: &'a str,
        context: &'a str,
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let (host, name) = forge_of(repo)?;
            let out = run(&stamp_call(&host, &name, sha, context), &name).await?;

            if !out.status.success() {
                let said = String::from_utf8_lossy(&out.stderr);
                return Err(tail(&said).to_owned());
            }

            Ok(())
        })
    }

    fn stampers<'a>(
        &'a self,
        repo: &'a TeamRepo,
        sha: &'a str,
        context: &'a str,
    ) -> BoxFuture<'a, Result<Vec<String>, String>> {
        Box::pin(async move {
            let (host, name) = forge_of(repo)?;
            let out = listed(repo, "statuses", &stamps_call(&host, &name, sha), &name).await?;

            parse_stampers(&out, context).map_err(|e| e.to_string())
        })
    }
}

/// What `call`, which lists `what` of `repo` (`name` on its forge), printed.
async fn listed(
    repo: &TeamRepo,
    what: &str,
    call: &ForgeCall,
    name: &RepoName,
) -> Result<String, String> {
    let out = run(call, name).await?;

    if !out.status.success() {
        let said = String::from_utf8_lossy(&out.stderr);
        return Err(format!(
            "Could not list the {what} of {repo}: {}",
            tail(&said)
        ));
    }

    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// `call` as a process. `gh` runs as the user's account for `repo`'s owner,
/// as [`ForgeCall::render`] has it in a shell: that owner's token when the
/// user gave one, else the default account's.
pub(super) fn forge_command(call: &ForgeCall, repo: &RepoName) -> Command {
    let mut command = command(call.program, &call.args);
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let owner = github_owner_var(repo.owner()).and_then(|var| spawn_env::secret(&var));

    if call.program == "gh"
        && let Some(token) = owner
    {
        command.env(GITHUB_ENV_VAR, token.expose());
    }

    command
}

pub(super) fn forge_of(repo: &TeamRepo) -> Result<(IssueHost, RepoName), String> {
    let host = IssueHost::of_hostname(repo.hostname())
        .ok_or_else(|| format!("{} is not a forge Beep can ask", repo.host))?;
    let name = repo
        .repo
        .parse()
        .map_err(|_| format!("{repo} is not an owner/repository"))?;

    Ok((host, name))
}

async fn run(call: &ForgeCall, repo: &RepoName) -> Result<Output, String> {
    let mut command = forge_command(call, repo);

    timeout(FORGE_LIMIT, command.output())
        .await
        .map_err(|_| format!("{} took too long", call.program))?
        .map_err(|e| format!("Could not start {}: {e}", call.program))
}
