//! The desktop's copy of each machine's search index, one file per machine,
//! kept current over the machine's `FollowIndex` stream. Cmd+K searches the
//! copies, so it answers at once and finds sessions of offline machines too.

use super::search::Found;
use crate::fleet::Fleet;
use anyhow::{Context, bail};
use beep_fleet::alerts::each_machine;
use beep_node::agents::search::IndexCopy;
use beep_proto::node::agent::{SESSIONS_PROTOCOL, SessionsReply, SessionsRequest};
use futures::StreamExt;
use iroh::EndpointId;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager};
use tokio::{task::spawn_blocking, time::sleep};

/// How long a machine that could not be followed is left before the next
/// try, doubling up to `RETRY_MOST`.
const RETRY_FIRST: Duration = Duration::from_secs(5);
const RETRY_MOST: Duration = Duration::from_secs(600);

pub(crate) struct Copies {
    dir: PathBuf,
    held: Mutex<HashMap<EndpointId, Arc<Mutex<IndexCopy>>>>,
}

impl Copies {
    /// Opens the copies kept under the app's `data` folder, so offline
    /// machines are searched too.
    pub fn load(data: &Path) -> Self {
        // The index this desktop built itself before nodes kept their own.
        for old in ["search.sqlite", "search.sqlite-wal", "search.sqlite-shm"] {
            let _ = fs::remove_file(data.join(old));
        }

        let copies = Self {
            dir: data.join("search-copies"),
            held: Mutex::default(),
        };
        let kept = fs::read_dir(&copies.dir).into_iter().flatten().flatten();

        for machine in kept.filter_map(|entry| machine_of(&entry.path())) {
            if let Err(e) = copies.copy(machine) {
                log::warn!("dropping the search copy of {machine}: {e:#}");
                let _ = fs::remove_file(copies.path(machine));
            }
        }

        copies
    }

    /// The hits of every copy.
    fn find(&self, query: &str) -> anyhow::Result<Vec<Found>> {
        let held: Vec<_> = self
            .held()
            .iter()
            .map(|(id, copy)| (*id, copy.clone()))
            .collect();
        let mut found = Vec::new();

        for (machine, copy) in held {
            let hits = lock(&copy).find(query)?;
            found.extend(hits.into_iter().map(|hit| Found {
                machine: machine.to_string(),
                hit,
            }));
        }

        Ok(found)
    }

    fn copy(&self, machine: EndpointId) -> anyhow::Result<Arc<Mutex<IndexCopy>>> {
        if let Some(copy) = self.held().get(&machine) {
            return Ok(copy.clone());
        }

        fs::create_dir_all(&self.dir)?;
        let copy = Arc::new(Mutex::new(IndexCopy::open(&self.path(machine))?));
        Ok(self.held().entry(machine).or_insert(copy).clone())
    }

    /// Drops the copies of machines no longer in the mesh.
    fn keep(&self, members: &[EndpointId]) {
        let gone: Vec<EndpointId> = self
            .held()
            .keys()
            .filter(|id| !members.contains(id))
            .copied()
            .collect();

        for machine in gone {
            self.held().remove(&machine);
            let _ = fs::remove_file(self.path(machine));
        }
    }

    fn path(&self, machine: EndpointId) -> PathBuf {
        self.dir.join(format!("{machine}.sqlite"))
    }

    fn held(&self) -> MutexGuard<'_, HashMap<EndpointId, Arc<Mutex<IndexCopy>>>> {
        lock(&self.held)
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn machine_of(path: &Path) -> Option<EndpointId> {
    (path.extension()? == "sqlite").then_some(())?;
    path.file_stem()?.to_str()?.parse().ok()
}

/// Follows every online machine's index while a mesh is open.
pub(crate) async fn follow(app: AppHandle) {
    let fleet = app.state::<Arc<Fleet>>().inner().clone();
    let copies = app.state::<Arc<Copies>>().inner().clone();

    loop {
        let mesh = match fleet.ready().await {
            Ok(mesh) => mesh,
            Err(e) => {
                log::info!("no mesh to follow search indexes in: {e}");
                sleep(RETRY_MOST).await;
                continue;
            }
        };
        let members: Vec<EndpointId> = mesh
            .code_hosts()
            .into_iter()
            .map(|status| status.device.id)
            .collect();
        copies.keep(&members);
        // Ends when the mesh closes, such as on joining another.
        each_machine(&mesh, |machine| {
            tokio::spawn(follow_machine(app.clone(), copies.clone(), machine))
        })
        .await;
    }
}

/// Keeps `machine`'s copy current while the machine is online.
async fn follow_machine(app: AppHandle, copies: Arc<Copies>, machine: EndpointId) {
    let mut wait = RETRY_FIRST;

    loop {
        match copy_from(&app, &copies, machine).await {
            Ok(true) => wait = RETRY_FIRST,
            Ok(false) => {}
            Err(e) => log::debug!("following the search index of {machine}: {e:#}"),
        }

        sleep(wait).await;
        wait = (wait * 2).min(RETRY_MOST);
    }
}

/// Applies the rows `machine` sends until its stream ends. True when some
/// came, so the next try need not wait long.
async fn copy_from(
    app: &AppHandle,
    copies: &Arc<Copies>,
    machine: EndpointId,
) -> anyhow::Result<bool> {
    let copy = copies.copy(machine)?;
    let held = copy.clone();
    let (epoch, after) = spawn_blocking(move || lock(&held).position()).await??;
    let fleet = app.state::<Arc<Fleet>>().inner().clone();
    let client = fleet.client(machine).await?;
    let mut replies = Box::pin(
        client
            .follow::<_, SessionsReply>(
                SESSIONS_PROTOCOL,
                &SessionsRequest::FollowIndex { epoch, after },
            )
            .await?,
    );
    let mut heard = false;

    while let Some(reply) = replies.next().await {
        match reply? {
            SessionsReply::IndexRows { epoch, rows, .. } => {
                heard = true;
                let added = !rows.is_empty();
                let held = copy.clone();
                spawn_blocking(move || lock(&held).apply(&epoch, &rows))
                    .await?
                    .context("saving the rows")?;

                if added {
                    let _ = app.emit("palette-text-changed", ());
                }
            }
            SessionsReply::Error { message } => bail!(message),
            _ => {}
        }
    }

    Ok(heard)
}

/// The hits of every copy, for the window.
pub(super) async fn find(app: &AppHandle, query: &str) -> anyhow::Result<Vec<Found>> {
    let copies = app.state::<Arc<Copies>>().inner().clone();
    let query = query.to_owned();
    spawn_blocking(move || copies.find(&query)).await?
}
