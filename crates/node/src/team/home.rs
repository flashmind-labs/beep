//! This mesh as the team link sees it: its teams, contacts and the projects
//! they are about, kept current from the mesh's state. Rosters and invites
//! the link takes go to a desktop, since only desktops write the state.

use crate::{
    process::spawn_env,
    process::{tail, tool_command},
};
use beep_mesh::{Key, Mesh};
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
    GITHUB_ENV_VAR, lenient,
    node::{
        mesh_state::{Place, ProjectRecord, StateValue},
        tools::TeamInviteArgs,
    },
    team::{ContactRecord, ForgeLogin, TeamRecord, TeamRepo},
};
use futures::future::BoxFuture;
use iroh::{EndpointAddr, EndpointId};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::{Output, Stdio},
    time::{Duration, SystemTime},
};
use tokio::{
    process::Command,
    time::timeout,
};

/// How long a forge call or a branch read may take.
pub(super) const FORGE_LIMIT: Duration = Duration::from_secs(30);
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

/// Whether `record` names this mesh: one a teammate removed it from does
/// not, and is as good as left until a desktop deletes it.
pub(super) fn on_team(record: &TeamRecord, mesh: &Mesh) -> bool {
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

/// What the forge says about a team's people. In tests it answers without one.
pub(crate) trait TeamForge: Send + Sync + 'static {
    /// Whether `login` may push to `repo`.
    fn collaborator<'a>(
        &'a self,
        repo: &'a TeamRepo,
        login: &'a ForgeLogin,
    ) -> BoxFuture<'a, Result<bool, String>>;

    /// The account this machine is signed in with on `repo`'s host.
    fn login<'a>(&'a self, repo: &'a TeamRepo) -> BoxFuture<'a, Result<ForgeLogin, String>>;

    /// `repo`'s open issues.
    fn open_issues<'a>(
        &'a self,
        repo: &'a TeamRepo,
    ) -> BoxFuture<'a, Result<Vec<OpenIssue>, String>>;

    /// `repo`'s open issues assigned to `login`, every page.
    fn assigned_issues<'a>(
        &'a self,
        repo: &'a TeamRepo,
        login: &'a ForgeLogin,
    ) -> BoxFuture<'a, Result<Vec<OpenIssue>, String>>;

    /// Whether `login` assigned issue `number` of `repo` to themselves, by its latest
    /// assignment of them.
    fn self_assigned<'a>(
        &'a self,
        repo: &'a TeamRepo,
        number: u64,
        login: &'a ForgeLogin,
    ) -> BoxFuture<'a, Result<bool, String>>;

    /// The branches of `repo`'s pull requests, open or closed, that may come from `branch`:
    /// `branch` among them when one does.
    fn pull_heads<'a>(
        &'a self,
        repo: &'a TeamRepo,
        branch: &'a str,
    ) -> BoxFuture<'a, Result<Vec<String>, String>>;

    /// Marks commit `sha` of `repo` with the status `context`, as this
    /// machine's account: only one who may push can.
    fn stamp<'a>(
        &'a self,
        repo: &'a TeamRepo,
        sha: &'a str,
        context: &'a str,
    ) -> BoxFuture<'a, Result<(), String>>;

    /// The logins that marked commit `sha` of `repo` with a status whose
    /// context starts with `context`.
    fn stampers<'a>(
        &'a self,
        repo: &'a TeamRepo,
        sha: &'a str,
        context: &'a str,
    ) -> BoxFuture<'a, Result<Vec<String>, String>>;
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
    let mut command = tool_command(call.program, &call.args);
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
