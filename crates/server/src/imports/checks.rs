//! Finds an imported project's checkouts on the user's other machines, by `origin` remote, through
//! the found-folders scan. Looks run in the background under [`Checkouts`], bounded per machine,
//! server-wide and in queue.

use super::{remove, repo::same_repo};
use crate::{
    app::App,
    credentials::ForgejoUrl,
    lock,
    protocol::{
        ProjectMachine,
        node::agent::{SESSIONS_PROTOCOL, SessionFolder},
    },
    store::{ImportRow, NewImport, NodeRow},
    user::User,
};
use anyhow::Context;
use chrono::{DateTime, TimeDelta, Utc};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, Weak},
    time::Duration,
};
use tokio::{sync::Semaphore, task::JoinSet, time::timeout};

/// The most checkouts one machine names in a look.
const FOLDERS_PER_MACHINE: u32 = 200;
/// The longest one machine's look may take.
const LOOK_LIMIT: Duration = Duration::from_secs(60);
/// How long a look at a machine with a project folder stands before the page asks again.
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

/// Drops the machine from projects whose folder is gone, records its checkouts, and returns the count.
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
    if !online.offers(SESSIONS_PROTOCOL) {
        return Ok(0);
    }
    if user.imports().await?.is_empty() {
        return Ok(0);
    }
    let started = Utc::now();
    let folders = timeout(LOOK_LIMIT, online.found_folders(FOLDERS_PER_MACHINE))
        .await
        .context("the node did not answer in time")??;
    let servers = forgejo_servers(app, user).await?;
    record(user, endpoint, &folders, &servers, started).await
}

/// The Forgejo servers the user has a token for.
pub(super) async fn forgejo_servers(app: &App, user: &User) -> anyhow::Result<Vec<ForgejoUrl>> {
    Ok(super::service::forgejo_of(
        &user.git_credentials(&app.credential_key).await?,
    ))
}

/// Records `folders` found on `endpoint` as new project folders there; returns how many.
pub(crate) async fn record(
    user: &User,
    endpoint: &str,
    folders: &[SessionFolder],
    servers: &[ForgejoUrl],
    started: DateTime<Utc>,
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
    user.save_checkout_check(endpoint, started).await?;
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
    /// Online machines without a checkout not looked through since the import, by key.
    pub(crate) to_check: Vec<String>,
}

/// What a project's page lists per machine; looks before the first import do not count.
pub(crate) fn machines(rows: &[ImportRow], machines: &[Machine<'_>]) -> Page {
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
        if folder.is_none() && checked_at.is_none() && machine.lookable && !checking {
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
        protocol::node::agent::AgentSessions,
        watchers::testing::{Harness, Scripted},
    };

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
    fn the_page_lists_every_machine_and_asks_for_the_unchecked_ones() {
        let since = Utc::now() - chrono::Duration::hours(1);
        let mut rows = vec![import(
            "acme/widget",
            "mac",
            "/Users/user/Projects/acme/widget",
        )];
        rows[0].imported_at = since;
        let mac = node("mac", "MacBook Air");
        let gpu = node("gpu", "gpu-box");
        let build = node("build", "build-box");
        let old = node("old", "old-box");
        let mini = node("mini", "mini-box");
        let at = |row, online, checked_at, checking| Machine {
            row,
            online,
            lookable: online,
            checking,
            checked_at,
        };
        let listed = [
            at(&old, false, None, true),
            at(
                &build,
                true,
                Some(since + chrono::Duration::minutes(5)),
                false,
            ),
            at(
                &gpu,
                true,
                Some(since - chrono::Duration::minutes(5)),
                false,
            ),
            at(&mini, true, None, true),
            at(&mac, true, None, false),
        ];
        let Page {
            machines: page,
            to_check,
        } = machines(&rows, &listed);
        let names: Vec<&str> = page.iter().map(|m| m.machine_name.as_str()).collect();
        assert_eq!(
            names,
            ["MacBook Air", "build-box", "gpu-box", "mini-box", "old-box"]
        );
        assert_eq!(
            page[0].folder.as_deref(),
            Some("/Users/user/Projects/acme/widget")
        );
        assert!(page[1].checked_at.is_some() && !page[1].checking);
        // Looked through before the import: that look does not count.
        assert!(page[2].checked_at.is_none() && page[2].checking);
        assert!(page[3].checking);
        assert!(!page[4].online && !page[4].checking);
        assert_eq!(to_check, ["gpu"]);
    }

    #[tokio::test]
    async fn a_look_links_the_checkout_and_notes_when() {
        let harness = Harness::new(Scripted::default()).await;
        let user = &harness.alice;
        user.save_import(NewImport {
            project: "acme/widget",
            endpoint: "laptop",
            folder: "/home/user/projects/acme/widget",
            url: "https://github.com/acme/widget",
        })
        .await
        .expect("saved");
        // A machine that connects later has a checkout over ssh.
        let started = Utc::now();
        let folders = [found(
            "/home/user/code/widget",
            "git@github.com:acme/widget.git",
            9,
        )];
        let linked = record(user, "build", &folders, &[], started)
            .await
            .expect("recorded");
        assert_eq!(linked, 1);
        let again = record(user, "build", &folders, &[], Utc::now())
            .await
            .expect("recorded");
        assert_eq!(again, 0);
        let places: Vec<(String, String)> = user
            .imports()
            .await
            .expect("listed")
            .into_iter()
            .map(|row| (row.endpoint, row.folder))
            .collect();
        assert_eq!(
            places,
            [
                ("build".into(), "/home/user/code/widget".into()),
                ("laptop".into(), "/home/user/projects/acme/widget".into()),
            ]
        );
        assert!(
            user.checkout_checks()
                .await
                .expect("read")
                .contains_key("build")
        );
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
        } = machines(std::slice::from_ref(&imported), &listed);
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

    #[test]
    fn a_machine_with_the_folder_is_looked_at_again_after_a_while() {
        let now = Utc::now();
        let rows = [
            import("acme/widget", "mac", "/Users/user/widget"),
            import("acme/widget", "build", "/srv/widget"),
            import("acme/widget", "old", "/srv/widget"),
            import("acme/widget", "busy", "/srv/widget"),
        ];
        let (mac, build, old, busy, gpu) = (
            node("mac", "MacBook Air"),
            node("build", "build-box"),
            node("old", "old-box"),
            node("busy", "busy-box"),
            node("gpu", "gpu-box"),
        );
        let at = |row, online, checked_at, checking| Machine {
            row,
            online,
            lookable: online,
            checking,
            checked_at,
        };
        let listed = [
            at(&mac, true, Some(now - TimeDelta::minutes(1)), false),
            at(&build, true, Some(now - TimeDelta::hours(1)), false),
            at(&old, false, None, false),
            at(&busy, true, None, true),
            at(&gpu, true, None, false),
        ];
        assert_eq!(stale(&rows, &listed, now), ["build"]);
    }
}
