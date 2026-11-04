//! Finds an imported project's checkouts on the user's other machines, by `origin` remote: first at
//! the paths its folders on other machines suggest, then through the found-folders scan. Looks run
//! in the background under [`Checkouts`], bounded per machine, server-wide and in queue.

use super::remove;
use crate::{
    account::user::User,
    app::App,
    lock,
    nodes::SHELL,
    protocol::{
        ProjectMachine,
        node::agent::{SESSIONS_PROTOCOL, SessionFolder},
    },
    store::NodeRow,
};
use anyhow::Context;
use beep_projects::branches::run_script;
use beep_projects::imports::preview::{parse_remotes, remotes_script};
use beep_projects::imports::records::{ImportRow, NewImport};
use beep_projects::imports::{folder::FolderPath, repo::same_repo};
use beep_projects::{Shell, model::ForgejoUrl};
use chrono::{DateTime, TimeDelta, Utc};
use std::{
    cmp::Reverse,
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, Weak},
    time::Duration,
};
use tokio::{sync::Semaphore, task::JoinSet, time::timeout};

/// The most checkouts one machine names in a look.
const FOLDERS_PER_MACHINE: u32 = 800;
/// The longest one machine's look may take.
const LOOK_LIMIT: Duration = Duration::from_secs(60);
/// The most suggested paths tried in a look.
const SIMILAR_PATHS: usize = 40;
/// The longest trying them may take.
const SIMILAR_LIMIT: Duration = Duration::from_secs(10);
/// How long a look at a machine stands before the page asks again.
const RECHECK: TimeDelta = TimeDelta::minutes(10);
/// Machines looked through at once, server-wide.
const AT_ONCE: usize = 4;
/// Machines running or queued; past it a look is dropped and the page asks again.
const MAX_PENDING: usize = 64;

/// One machine of one user.
type MachineKey = (String, String);

/// The looks in flight, and the machines they are for.
pub struct Checkouts {
    /// Machines being looked through, and whether another look was asked for meanwhile.
    running: Arc<Mutex<HashMap<MachineKey, bool>>>,
    permits: Arc<Semaphore>,
    tasks: Mutex<JoinSet<()>>,
}

impl Default for Checkouts {
    fn default() -> Self {
        Self {
            running: Arc::default(),
            permits: Arc::new(Semaphore::new(AT_ONCE)),
            tasks: Mutex::new(JoinSet::new()),
        }
    }
}

/// Takes a machine off the running list when its task ends, even by panic or abort.
struct Running {
    running: Arc<Mutex<HashMap<MachineKey, bool>>>,
    key: MachineKey,
    /// The machine left the list already.
    left: bool,
}

impl Running {
    /// Whether another look was asked for; if not, the machine leaves the list in the same step.
    fn again(&mut self) -> bool {
        let mut running = lock(&self.running);
        if let Some(again) = running.get_mut(&self.key)
            && *again
        {
            *again = false;
            return true;
        }
        running.remove(&self.key);
        self.left = true;
        false
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        if !self.left {
            lock(&self.running).remove(&self.key);
        }
    }
}

impl Checkouts {
    /// Whether the user's machine `endpoint` is being looked through now.
    pub(crate) fn is_checking(&self, user: &str, endpoint: &str) -> bool {
        lock(&self.running).contains_key(&(user.to_owned(), endpoint.to_owned()))
    }

    /// Looks through machine `endpoint` in the background, again after a running look; `false` when dropped.
    pub(crate) fn check(&self, app: &Arc<App>, user: &Arc<User>, endpoint: &str) -> bool {
        let key = (user.id().to_owned(), endpoint.to_owned());
        {
            let mut running = lock(&self.running);
            if let Some(again) = running.get_mut(&key) {
                *again = true;
                return true;
            }
            let mut tasks = lock(&self.tasks);
            while let Some(ended) = tasks.try_join_next() {
                if let Err(e) = ended
                    && e.is_panic()
                {
                    log::warn!("checkouts: a look panicked: {e}");
                }
            }
            if tasks.len() >= MAX_PENDING {
                log::warn!("checkouts: dropped a look, too many waiting; node={endpoint}");
                return false;
            }
            running.insert(key.clone(), false);
            let mut guard = Running {
                running: self.running.clone(),
                key,
                left: false,
            };
            let app = Arc::downgrade(app);
            let user = user.clone();
            let permits = self.permits.clone();
            let endpoint = endpoint.to_owned();
            tasks.spawn(async move {
                let Ok(_permit) = permits.acquire_owned().await else {
                    return;
                };
                loop {
                    look(&app, &user, &endpoint).await;
                    if !guard.again() {
                        break;
                    }
                }
            });
        }
        true
    }

    /// Waits for every look under way to end.
    #[cfg(test)]
    pub(crate) async fn wait_idle(&self) {
        let mut tasks = std::mem::take(&mut *lock(&self.tasks));
        while tasks.join_next().await.is_some() {}
    }

    /// Looks through each of `endpoints`, as after an import.
    pub(crate) fn check_all(&self, app: &Arc<App>, user: &Arc<User>, endpoints: &[String]) {
        for endpoint in endpoints {
            self.check(app, user, endpoint);
        }
    }
}

/// One look, its failure logged; the machine is looked through again later.
async fn look(app: &Weak<App>, user: &Arc<User>, endpoint: &str) {
    let Some(app) = app.upgrade() else {
        return;
    };
    match look_through(&app, user, endpoint).await {
        Ok(0) => {}
        Ok(linked) => log::info!(
            "checkouts found; user={} node={endpoint} linked={linked}",
            user.id()
        ),
        Err(e) => log::info!("checkouts not looked for; node={endpoint} error={e:#}"),
    }
}

/// Drops the machine from projects whose folder is gone, records its checkouts at suggested paths,
/// then those the full look finds, and returns that last count.
async fn look_through(app: &Arc<App>, user: &Arc<User>, endpoint: &str) -> anyhow::Result<usize> {
    let Some(online) = app
        .nodes
        .online(endpoint)
        .filter(|online| online.user == user.id())
    else {
        return Ok(0);
    };
    match remove::drop_missing(app, user, &online, endpoint).await {
        Ok(0) => {}
        Ok(dropped) => log::info!(
            "checkouts gone; user={} node={endpoint} dropped={dropped}",
            user.id()
        ),
        // A look that failed proves nothing is gone.
        Err(e) => log::info!("checkouts not looked at; node={endpoint} error={e:#}"),
    }
    if user.imports().await?.is_empty() {
        return Ok(0);
    }
    let servers = forgejo_servers(app, user).await?;
    if online.offers(SHELL) {
        match link_similar(user, &online, endpoint, &servers).await {
            Ok(0) => {}
            Ok(linked) => log::info!(
                "checkouts found at similar paths; user={} node={endpoint} linked={linked}",
                user.id()
            ),
            // The full look still finds them.
            Err(e) => log::info!("similar paths not tried; node={endpoint} error={e:#}"),
        }
    }
    if !online.offers(SESSIONS_PROTOCOL) {
        return Ok(0);
    }
    let started = Utc::now();
    let folders = timeout(LOOK_LIMIT, online.found_folders(FOLDERS_PER_MACHINE))
        .await
        .context("the node did not answer in time")??;
    record(user, endpoint, &folders, &servers, started).await
}

/// The Forgejo servers the user has a token for.
pub(super) async fn forgejo_servers(app: &App, user: &User) -> anyhow::Result<Vec<ForgejoUrl>> {
    Ok(beep_projects::imports::hosts::forgejo_of(
        &user.git_credentials(&app.credential_key).await?,
    ))
}

/// Tries the paths [`similar_paths`] suggests through `shell` and records the checkouts there.
async fn link_similar(
    user: &User,
    shell: &dyn Shell,
    endpoint: &str,
    servers: &[ForgejoUrl],
) -> anyhow::Result<usize> {
    let paths = similar_paths(&user.imports().await?, &user.nodes().await?, endpoint);
    if paths.is_empty() {
        return Ok(0);
    }
    let out = run_script(shell, &remotes_script(&paths), SIMILAR_LIMIT).await?;
    let folders: Vec<SessionFolder> = parse_remotes(&out)
        .into_iter()
        .filter_map(|folder| {
            let (_, origin) = folder
                .remotes
                .into_iter()
                .find(|(name, _)| name == "origin")?;
            Some(SessionFolder {
                path: folder.folder,
                origin,
                agents: Vec::new(),
                last_activity: 0,
            })
        })
        .collect();
    link_found(user, endpoint, &folders, servers).await
}

/// `path` when it is absolute in the form a Unix machine takes, cleaned up.
fn absolute(path: &str) -> Option<String> {
    match FolderPath::parse(path) {
        Ok(Some(FolderPath::Absolute(path))) => Some(path),
        _ => None,
    }
}

/// Where projects not on `endpoint` may be, newest import first: their folders on other machines,
/// with that machine's home swapped for this one's. A path that does not parse, such as a Windows
/// one, suggests nothing.
fn similar_paths(imports: &[ImportRow], nodes: &[NodeRow], endpoint: &str) -> Vec<String> {
    let homes: HashMap<&str, String> = nodes
        .iter()
        .filter_map(|node| Some((node.endpoint.as_str(), absolute(&node.env.as_ref()?.home)?)))
        .collect();
    let Some(home) = homes.get(endpoint) else {
        return Vec::new();
    };
    let here: HashSet<&str> = imports
        .iter()
        .filter(|row| row.endpoint == endpoint)
        .map(|row| row.project.as_str())
        .collect();
    let mut elsewhere: Vec<&ImportRow> = imports
        .iter()
        .filter(|row| !here.contains(row.project.as_str()))
        .collect();
    elsewhere.sort_by_key(|row| Reverse(row.imported_at));
    let mut seen = HashSet::new();
    elsewhere
        .into_iter()
        .filter_map(|row| {
            let folder = absolute(&row.folder)?;
            let below = folder
                .strip_prefix(homes.get(row.endpoint.as_str())?.as_str())?
                .strip_prefix('/')?;
            Some(format!("{}/{below}", home.trim_end_matches('/')))
        })
        .filter(|path| seen.insert(path.clone()))
        .take(SIMILAR_PATHS)
        .collect()
}

/// Records `folders` found on `endpoint` by the full look, and notes the look; returns how many are new.
pub(crate) async fn record(
    user: &User,
    endpoint: &str,
    folders: &[SessionFolder],
    servers: &[ForgejoUrl],
    started: DateTime<Utc>,
) -> anyhow::Result<usize> {
    let linked = link_found(user, endpoint, folders, servers).await?;
    user.save_checkout_check(endpoint, started).await?;
    Ok(linked)
}

/// Records `folders` on `endpoint` as new project folders there; returns how many.
async fn link_found(
    user: &User,
    endpoint: &str,
    folders: &[SessionFolder],
    servers: &[ForgejoUrl],
) -> anyhow::Result<usize> {
    // Read after the look, so a project imported during it is matched too.
    let imports = user.imports().await?;
    let mut linked = 0;
    for link in links(&imports, endpoint, folders, servers) {
        let recorded = user
            .link_import(NewImport {
                project: &link.project,
                endpoint,
                folder: &link.folder,
                url: &link.url,
            })
            .await?;
        if recorded {
            linked += 1;
        }
    }
    Ok(linked)
}

/// A checkout to record as a project's folder.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Link {
    pub(crate) project: String,
    pub(crate) url: String,
    pub(crate) folder: String,
}

/// One checkout per project in `imports` without a folder on `endpoint`, the one used last.
pub(crate) fn links(
    imports: &[ImportRow],
    endpoint: &str,
    folders: &[SessionFolder],
    servers: &[ForgejoUrl],
) -> Vec<Link> {
    let here: HashSet<&str> = imports
        .iter()
        .filter(|row| row.endpoint == endpoint)
        .map(|row| row.project.as_str())
        .collect();
    let mut seen: HashSet<&str> = HashSet::new();
    let mut out = Vec::new();
    for row in imports {
        if here.contains(row.project.as_str()) || !seen.insert(row.project.as_str()) {
            continue;
        }
        let best = folders
            .iter()
            .filter(|folder| same_repo(&row.url, &folder.origin, servers))
            .max_by(|a, b| {
                a.last_activity
                    .cmp(&b.last_activity)
                    .then_with(|| b.path.cmp(&a.path))
            });
        if let Some(folder) = best {
            out.push(Link {
                project: row.project.clone(),
                url: row.url.clone(),
                folder: folder.path.clone(),
            });
        }
    }
    out
}

/// One of the user's machines, as `machines` reads it.
pub(crate) struct Machine<'a> {
    pub(crate) row: &'a NodeRow,
    pub(crate) online: bool,
    /// It can be looked through: online with a node new enough.
    pub(crate) lookable: bool,
    pub(crate) checking: bool,
    pub(crate) checked_at: Option<DateTime<Utc>>,
}

/// A project's page: its machines, and those to look through now.
pub(crate) struct Page {
    pub(crate) machines: Vec<ProjectMachine>,
    /// Online machines without a checkout not looked through lately, by key.
    pub(crate) to_check: Vec<String>,
}

/// What a project's page lists per machine; looks before the first import do not count, and one
/// older than `RECHECK` is repeated: a checkout may have appeared, or the node not known it yet.
pub(crate) fn machines(rows: &[ImportRow], machines: &[Machine<'_>], now: DateTime<Utc>) -> Page {
    let since = rows.iter().map(|row| row.imported_at).min();
    let mut listed = Vec::new();
    let mut to_check = Vec::new();
    for machine in machines {
        let folder = rows
            .iter()
            .find(|row| row.endpoint == machine.row.endpoint)
            .map(|row| row.folder.clone());
        let checked_at = machine
            .checked_at
            .filter(|at| since.is_none_or(|since| *at >= since));
        // A look left over from before it went offline answers nothing; it
        // is looked through again when it connects.
        let mut checking = machine.checking && machine.online;
        let due = checked_at.is_none_or(|at| at + RECHECK < now);
        if folder.is_none() && due && machine.lookable && !checking {
            to_check.push(machine.row.endpoint.clone());
            checking = true;
        }
        listed.push(ProjectMachine {
            machine_id: machine.row.endpoint.clone(),
            machine_name: machine.row.name.clone(),
            online: machine.online,
            folder,
            checking,
            checked_at: checked_at.map(Into::into),
        });
    }
    listed.sort_by(|a, b| {
        b.folder
            .is_some()
            .cmp(&a.folder.is_some())
            .then_with(|| b.online.cmp(&a.online))
            .then_with(|| a.machine_name.cmp(&b.machine_name))
    });
    Page {
        machines: listed,
        to_check,
    }
}

/// Online machines with a project folder not looked through for `RECHECK`, by key.
pub(crate) fn stale(
    rows: &[ImportRow],
    machines: &[Machine<'_>],
    now: DateTime<Utc>,
) -> Vec<String> {
    machines
        .iter()
        .filter(|machine| {
            machine.lookable
                && !machine.checking
                && rows.iter().any(|row| row.endpoint == machine.row.endpoint)
                && machine.checked_at.is_none_or(|at| at + RECHECK < now)
        })
        .map(|machine| machine.row.endpoint.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        memory::watchers::testing::{Harness, Scripted},
        protocol::node::{EnvReport, agent::AgentSessions},
        store::NewNode,
    };
    use async_trait::async_trait;
    use beep_projects::ShellOutput;
    use iroh::SecretKey;
    use std::{path::Path, process::Command};

    fn import(project: &str, endpoint: &str, folder: &str) -> ImportRow {
        ImportRow {
            project: project.into(),
            endpoint: endpoint.into(),
            folder: folder.into(),
            url: format!("https://github.com/{project}"),
            imported_at: Utc::now(),
            fork: None,
        }
    }

    fn found(path: &str, origin: &str, last: u64) -> SessionFolder {
        SessionFolder {
            path: path.into(),
            origin: origin.into(),
            agents: vec![AgentSessions {
                agent: "claude".into(),
                sessions: 1,
            }],
            last_activity: last,
        }
    }

    fn node(endpoint: &str, name: &str) -> NodeRow {
        NodeRow {
            endpoint: endpoint.into(),
            name: name.into(),
            version: "0.1.0".into(),
            env: None,
            seen_at: Utc::now(),
        }
    }

    #[test]
    fn a_projects_checkout_on_another_machine_links_by_its_remote() {
        let imports = [
            import("acme/widget", "laptop", "/home/user/projects/acme/widget"),
            import("acme/docs", "laptop", "/home/user/projects/acme/docs"),
            import("acme/site", "build", "/srv/site"),
        ];
        let folders = [
            found("/home/user/old/widget", "git@github.com:Acme/Widget.git", 5),
            found(
                "/home/user/code/widget",
                "https://github.com/acme/widget",
                9,
            ),
            found("/home/user/fork", "git@github.com:me/widget.git", 20),
            found("/home/user/site", "git@github.com:acme/site.git", 30),
        ];
        let on_build = links(&imports, "build", &folders, &[]);
        assert_eq!(
            on_build,
            [Link {
                project: "acme/widget".into(),
                url: "https://github.com/acme/widget".into(),
                folder: "/home/user/code/widget".into(),
            }],
            "the busiest checkout, and acme/site keeps its folder on build"
        );
        let on_laptop: Vec<String> = links(&imports, "laptop", &folders, &[])
            .into_iter()
            .map(|link| link.folder)
            .collect();
        assert_eq!(on_laptop, ["/home/user/site"]);
    }

    #[test]
    fn a_machine_looked_through_long_ago_is_looked_through_again() {
        let imported = import("acme/widget", "laptop", "/home/user/projects/acme/widget");
        let build = node("build", "build-box");
        let now = imported.imported_at + RECHECK * 3;
        let listed = |checked_at| {
            [Machine {
                row: &build,
                online: true,
                lookable: true,
                checking: false,
                checked_at: Some(checked_at),
            }]
        };
        let rows = std::slice::from_ref(&imported);

        let recent = machines(rows, &listed(now - TimeDelta::minutes(1)), now);
        assert!(recent.to_check.is_empty());
        assert!(!recent.machines[0].checking);

        let old = machines(rows, &listed(now - RECHECK - TimeDelta::minutes(1)), now);
        assert_eq!(old.to_check, ["build"]);
        assert!(old.machines[0].checking);
    }

    #[tokio::test]
    async fn a_project_imported_after_a_look_is_looked_for_again() {
        let harness = Harness::new(Scripted::default()).await;
        let user = &harness.alice;
        // build was looked through before anything was imported.
        let before = Utc::now() - chrono::Duration::minutes(10);
        record(user, "build", &[], &[], before)
            .await
            .expect("recorded");
        let imported = user
            .save_import(NewImport {
                project: "acme/widget",
                endpoint: "laptop",
                folder: "/home/user/projects/acme/widget",
                url: "https://github.com/acme/widget",
            })
            .await
            .expect("saved");
        let build = node("build", "build-box");
        let checks = user.checkout_checks().await.expect("read");
        let listed = [Machine {
            row: &build,
            online: true,
            lookable: true,
            checking: false,
            checked_at: checks.get("build").copied(),
        }];
        let Page {
            machines: page,
            to_check,
        } = machines(std::slice::from_ref(&imported), &listed, Utc::now());
        assert!(page[0].checking);
        assert_eq!(to_check, ["build"]);
        // The look the import starts finds it.
        let folders = [found(
            "/srv/widget",
            "https://github.com/acme/widget.git",
            3,
        )];
        assert_eq!(
            record(user, "build", &folders, &[], Utc::now())
                .await
                .expect("recorded"),
            1
        );
    }

    /// Runs commands here, as a node would.
    struct Local;

    #[async_trait]
    impl Shell for Local {
        async fn run(&self, command: &str, _limit: Duration) -> anyhow::Result<ShellOutput> {
            let ran = Command::new("sh").arg("-c").arg(command).output()?;
            Ok(ShellOutput {
                exit_code: ran.status.code(),
                timed_out: false,
                output: String::from_utf8_lossy(&ran.stdout).into_owned(),
            })
        }
    }

    fn checkout(folder: &Path, origin: &str) {
        std::fs::create_dir_all(folder).expect("a folder");
        for args in [
            &["init", "-q"][..],
            &["remote", "add", "origin", origin][..],
        ] {
            let ok = Command::new("git")
                .args(args)
                .current_dir(folder)
                .status()
                .expect("git runs")
                .success();
            assert!(ok, "git {args:?}");
        }
    }

    /// A machine of `user` with this home; returns its key.
    async fn machine(user: &User, home: &str) -> String {
        let endpoint = SecretKey::generate().public();
        user.node_connected(NewNode {
            endpoint,
            name: "box",
            version: "0.1.0",
        })
        .await
        .expect("connected");
        let env = EnvReport {
            home: home.into(),
            ..EnvReport::default()
        };
        user.save_node_env(&endpoint, &env).await.expect("saved");
        endpoint.to_string()
    }

    /// Alice with acme/widget at `code/widget` under a laptop's home, and another machine holding a
    /// checkout of `origin` at that place under its own. Returns what was linked and that machine.
    async fn try_similar(harness: &Harness, origin: &str) -> (usize, String) {
        let user = &harness.alice;
        let laptop = machine(user, "/srv/someone").await;
        user.save_import(NewImport {
            project: "acme/widget",
            endpoint: &laptop,
            folder: "/srv/someone/code/widget",
            url: "https://github.com/acme/widget",
        })
        .await
        .expect("saved");
        let home = tempfile::tempdir().expect("a temp folder");
        checkout(&home.path().join("code/widget"), origin);
        let build = machine(user, &home.path().to_string_lossy()).await;
        let linked = link_similar(user, &Local, &build, &[])
            .await
            .expect("tried");
        (linked, build)
    }

    #[tokio::test]
    async fn a_similar_path_is_found_without_a_full_look() {
        let harness = Harness::new(Scripted::default()).await;
        let user = &harness.alice;
        let (linked, build) = try_similar(&harness, "git@github.com:Acme/Widget.git").await;
        assert_eq!(linked, 1);
        let imports = user.imports().await.expect("read");
        let here = imports
            .iter()
            .find(|row| row.endpoint == build)
            .expect("linked");
        assert!(here.folder.ends_with("/code/widget"), "{}", here.folder);
        assert!(
            !user
                .checkout_checks()
                .await
                .expect("read")
                .contains_key(&build),
            "the full look is still to come"
        );
    }

    #[tokio::test]
    async fn a_folder_at_a_similar_path_with_another_remote_is_not_linked() {
        let harness = Harness::new(Scripted::default()).await;
        let (linked, build) = try_similar(&harness, "git@github.com:acme/other.git").await;
        assert_eq!(linked, 0);
        let imports = harness.alice.imports().await.expect("read");
        assert!(imports.iter().all(|row| row.endpoint != build));
    }

    #[test]
    fn similar_paths_skip_what_does_not_parse_and_stay_bounded() {
        let with_home = |endpoint: &str, home: &str| NodeRow {
            env: Some(EnvReport {
                home: home.into(),
                ..EnvReport::default()
            }),
            ..node(endpoint, endpoint)
        };
        let nodes = [
            with_home("here", "/home/b"),
            with_home("linux", "/srv/a"),
            with_home("windows", r"C:\Users\a"),
        ];
        let imports = [
            import("acme/widget", "linux", "/srv/a/code/widget"),
            import("acme/outside", "linux", "/opt/outside"),
            import("acme/home", "linux", "/srv/a"),
            import("acme/win", "windows", r"C:\Users\a\code\win"),
            import("acme/kept", "linux", "/srv/a/code/kept"),
            import("acme/kept", "here", "/home/b/kept"),
        ];
        assert_eq!(
            similar_paths(&imports, &nodes, "here"),
            ["/home/b/code/widget"]
        );
        assert!(similar_paths(&imports, &nodes, "windows").is_empty());

        let many: Vec<ImportRow> = (0..SIMILAR_PATHS * 2)
            .map(|n| import(&format!("acme/r{n}"), "linux", &format!("/srv/a/r{n}")))
            .collect();
        assert_eq!(similar_paths(&many, &nodes, "here").len(), SIMILAR_PATHS);
    }

    #[tokio::test]
    async fn a_machine_looked_through_twice_at_once_looks_once_more_after() {
        let harness = Harness::new(Scripted::default()).await;
        let checks = &harness.app.checkouts;
        // Offline, so each look ends at once without recording.
        assert!(checks.check(&harness.app, &harness.alice, "build"));
        assert!(checks.is_checking("alice", "build"));
        assert!(checks.check(&harness.app, &harness.alice, "build"));
        let mut tasks = std::mem::take(&mut *lock(&checks.tasks));
        assert_eq!(tasks.len(), 1, "one task for the machine");
        while tasks.join_next().await.is_some() {}
        assert!(!checks.is_checking("alice", "build"));
    }
}
