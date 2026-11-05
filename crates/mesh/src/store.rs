//! Everything that blocks on fsync lives here, on one owner thread: the
//! device list file, the replicated state log and the minted tickets. Async
//! code sends it closures and awaits the answer, so a slow disk delays the
//! caller but never an async worker. Readers use the published `View`.

use crate::mesh::AddrNote;
use crate::state::{MESH_DIR, OLD_MESH_DIR, now_ms, private_dir, private_options, sync_dir};
use crate::{
    Applied, Device, DeviceKind, DeviceOp, Devices, Entry, Heads, JoinSecret, JoinTicket, Key,
    Merged, MeshError, State, WriteLimit,
};
use beep_proto::lenient;
use iroh::{EndpointAddr, EndpointId, SecretKey};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, PoisonError, RwLock};
use std::thread;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, watch};
use tracing::warn;

/// The largest value a device may write under one key.
pub(crate) const MAX_VALUE: usize = 1024 * 1024;
const QUEUE: usize = 256;
/// Where a mesh's files wait while another takes their place.
const ASIDE: &str = "mesh2.old";

fn remove_if_present(dir: &Path) -> Result<(), MeshError> {
    match fs::remove_dir_all(dir) {
        Err(e) if e.kind() != ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

/// What readers see: a snapshot rebuilt after every change.
pub(crate) struct View {
    pub devices: Devices,
    pub values: BTreeMap<Key, Vec<u8>>,
    /// Every key that was ever written, deleted ones included.
    pub held: BTreeSet<Key>,
}

pub(crate) type SharedView = Arc<RwLock<Arc<View>>>;

pub(crate) struct Core {
    key: SecretKey,
    dir: PathBuf,
    devices: Devices,
    state: State,
    tickets: Vec<JoinTicket>,
    view: SharedView,
    rev: Arc<watch::Sender<u64>>,
}

impl Core {
    /// Opens the files of a mesh `devices` describes and saves the list.
    pub(crate) fn start(dir: &Path, key: SecretKey, devices: Devices) -> Result<Core, MeshError> {
        let state = State::open(dir, &key, devices.mesh())?;
        let tickets = match fs::read(dir.join(MESH_DIR).join("tickets.bin")) {
            Ok(bytes) => lenient::from_slice(&bytes)?,
            Err(e) if e.kind() == ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e.into()),
        };
        let view = Arc::new(View {
            devices: devices.clone(),
            values: BTreeMap::new(),
            held: BTreeSet::new(),
        });
        let core = Core {
            key,
            dir: dir.to_owned(),
            devices,
            state,
            tickets,
            view: Arc::new(RwLock::new(view)),
            rev: Arc::new(watch::channel(0).0),
        };
        core.save_devices()?;
        core.publish();
        Ok(core)
    }

    /// Finishes or undoes a swap a crash cut short (see `swap_in`): once
    /// the new mesh's device list is on disk the old one goes, before that
    /// the old one is put back.
    pub(crate) fn recover(dir: &Path) -> Result<(), MeshError> {
        let (live, aside) = (dir.join(MESH_DIR), dir.join(ASIDE));
        if !aside.exists() {
            return Ok(());
        }
        if !live.join("devices.bin").exists() {
            remove_if_present(&live)?;
            fs::rename(&aside, &live)?;
        } else {
            remove_if_present(&aside)?;
        }
        sync_dir(dir)?;
        Ok(())
    }

    /// Starts a mesh in `dir` in place of the one there, which is kept
    /// aside until the new one is on disk. A crash leaves one of the two.
    pub(crate) fn swap_in(
        dir: &Path,
        key: SecretKey,
        devices: Devices,
        addrs: &[AddrNote],
    ) -> Result<Core, MeshError> {
        Self::recover(dir)?;
        let (live, aside) = (dir.join(MESH_DIR), dir.join(ASIDE));
        let had_old = live.exists();
        if had_old {
            fs::rename(&live, &aside)?;
            sync_dir(dir)?;
        }
        let started = Self::start(dir, key, devices).and_then(|core| {
            core.save_addrs(addrs)?;
            sync_dir(dir)?;
            Ok(core)
        });
        match started {
            Ok(core) => {
                remove_if_present(&aside)?;
                sync_dir(dir)?;
                Ok(core)
            }
            Err(e) => {
                if had_old {
                    remove_if_present(&live)?;
                    fs::rename(&aside, &live)?;
                }
                Err(e)
            }
        }
    }

    /// Removes the mesh in `dir`, one a swap left aside, and the folder of a
    /// mesh from an older version, which holds credentials in plain text.
    pub(crate) fn forget(dir: &Path) -> Result<(), MeshError> {
        remove_if_present(&dir.join(MESH_DIR))?;
        remove_if_present(&dir.join(OLD_MESH_DIR))?;
        remove_if_present(&dir.join(ASIDE))?;
        sync_dir(dir)?;
        Ok(())
    }

    pub(crate) fn exists(dir: &Path) -> bool {
        dir.join(MESH_DIR).join("devices.bin").exists()
    }

    pub(crate) fn load(dir: &Path) -> Result<Option<Devices>, MeshError> {
        let bytes = match fs::read(dir.join(MESH_DIR).join("devices.bin")) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == ErrorKind::NotFound => {
                if dir.join(OLD_MESH_DIR).join("devices.bin").exists() {
                    warn!(
                        "found a mesh from an older version in {}; it is not read and stays as it is. \
                         Join the mesh again with a new ticket.",
                        dir.join(OLD_MESH_DIR).display()
                    );
                }
                return Ok(None);
            }
            Err(e) => return Err(e.into()),
        };
        let ops: Vec<DeviceOp> = lenient::from_slice(&bytes)?;
        Ok(Some(Devices::from_ops(&ops)?.0))
    }

    pub(crate) fn load_addrs(dir: &Path) -> Vec<AddrNote> {
        fs::read(dir.join(MESH_DIR).join("peers.bin"))
            .ok()
            .and_then(|bytes| lenient::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub(crate) fn view(&self) -> SharedView {
        self.view.clone()
    }

    pub(crate) fn rev(&self) -> Arc<watch::Sender<u64>> {
        self.rev.clone()
    }

    fn write(&self, name: &str, bytes: &[u8], durable: bool) -> Result<(), MeshError> {
        let mesh_dir = self.dir.join(MESH_DIR);
        private_dir(&mesh_dir)?;
        let path = mesh_dir.join(name);
        let tmp = mesh_dir.join(format!("{name}.tmp"));
        let mut file = private_options()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp)?;
        file.write_all(bytes)?;
        if durable {
            file.sync_all()?;
        }
        fs::rename(&tmp, &path)?;
        if durable {
            sync_dir(&mesh_dir)?;
        }
        Ok(())
    }

    fn save_devices(&self) -> Result<(), MeshError> {
        self.write(
            "devices.bin",
            &rmp_serde::to_vec(&self.devices.ops())?,
            true,
        )
    }

    fn save_tickets(&self) -> Result<(), MeshError> {
        self.write("tickets.bin", &rmp_serde::to_vec(&self.tickets)?, true)
    }

    /// Rebuilds the snapshot and wakes everyone watching.
    fn publish(&self) {
        let values = self
            .state
            .scan("")
            .into_iter()
            .map(|(key, value)| (key, value.to_vec()))
            .collect();
        let view = View {
            devices: self.devices.clone(),
            values,
            held: self.state.held().into_iter().collect(),
        };
        *self.view.write().unwrap_or_else(PoisonError::into_inner) = Arc::new(view);
        self.rev.send_modify(|n| *n = n.wrapping_add(1));
    }

    fn me(&self) -> EndpointId {
        self.key.public()
    }

    fn desktop(&self) -> Result<(), MeshError> {
        let me = self.me();
        if self.devices.is_desktop(me) {
            Ok(())
        } else if self.devices.write_limit(&me) == WriteLimit::Refused {
            Err(MeshError::NotDesktop(me))
        } else {
            Err(MeshError::Removed(me))
        }
    }

    pub(crate) fn invite(
        &mut self,
        addr: EndpointAddr,
        kind: DeviceKind,
        ttl: Duration,
    ) -> Result<JoinTicket, MeshError> {
        self.desktop()?;
        let now = now_ms();
        let ttl_ms = u64::try_from(ttl.as_millis()).unwrap_or(u64::MAX);
        let ticket = JoinTicket {
            mesh: self.devices.mesh(),
            addr,
            kind,
            secret: JoinSecret::generate(),
            expires_ms: now.saturating_add(ttl_ms),
        };
        self.tickets.retain(|t| !t.expired(now));
        self.tickets.push(ticket.clone());
        self.save_tickets()?;
        Ok(ticket)
    }

    /// Spends the ticket `secret` for `joiner`. The spent ticket is on disk
    /// before the `Add` exists, so a crash cannot let it work twice.
    pub(crate) fn redeem(
        &mut self,
        secret: &JoinSecret,
        joiner: EndpointId,
        name: &str,
    ) -> Result<Vec<DeviceOp>, MeshError> {
        self.desktop()?;
        let refuse = |why: &str| MeshError::Refused(why.to_owned());
        let now = now_ms();
        let Some(at) = self.tickets.iter().position(|t| t.secret.verify(secret)) else {
            return Err(refuse("ticket not valid"));
        };
        let ticket = self.tickets.remove(at);
        self.tickets.retain(|t| !t.expired(now));
        self.save_tickets()?;
        if ticket.expired(now) {
            return Err(refuse("ticket expired"));
        }
        let device = Device {
            id: joiner,
            name: name.to_owned(),
            kind: ticket.kind,
            added_at_ms: now,
        };
        match self.devices.add(&self.key, device) {
            Ok(_) => {}
            Err(MeshError::Removed(_)) => return Err(refuse("this key was removed")),
            Err(e) => return Err(e),
        }
        self.save_devices()?;
        self.publish();
        Ok(self.devices.ops().to_vec())
    }

    pub(crate) fn remove(&mut self, id: EndpointId) -> Result<(), MeshError> {
        self.desktop()?;
        self.devices.remove(&self.key, id)?;
        self.save_devices()?;
        self.publish();
        Ok(())
    }

    pub(crate) fn set(&mut self, key: Key, value: Option<Vec<u8>>) -> Result<Entry, MeshError> {
        self.desktop()?;
        if value.as_ref().is_some_and(|v| v.len() > MAX_VALUE) {
            return Err(MeshError::TooLarge);
        }
        let entry = self.state.set(key, value)?;
        self.publish();
        Ok(entry)
    }

    pub(crate) fn merge(&mut self, ops: &[DeviceOp]) -> Result<Merged, MeshError> {
        let before = self.devices.ops().len();
        let merged = self.devices.merge(ops);
        if self.devices.ops().len() != before {
            self.save_devices()?;
            self.publish();
        }
        Ok(merged)
    }

    pub(crate) fn apply(&mut self, entries: Vec<Entry>) -> Result<Applied, MeshError> {
        let applied = self.state.apply(entries, &self.devices)?;
        if !applied.changed.is_empty() {
            self.publish();
        }
        Ok(applied)
    }

    pub(crate) fn heads(&self) -> Heads {
        self.state.heads()
    }

    pub(crate) fn since(&self, theirs: &Heads) -> Vec<Entry> {
        self.state.since(theirs)
    }

    pub(crate) fn ops(&self) -> Vec<DeviceOp> {
        self.devices.ops().to_vec()
    }

    /// Best effort: a lost address file only costs a rediscovery.
    pub(crate) fn save_addrs(&self, addrs: &[AddrNote]) -> Result<(), MeshError> {
        self.write("peers.bin", &rmp_serde::to_vec(addrs)?, false)
    }
}

type Job = Box<dyn FnOnce(&mut Core) + Send>;

enum Msg {
    Job(Job),
    /// Ends the thread, which answers once its `Core` is dropped.
    Stop(oneshot::Sender<()>),
}

/// The owner thread of a `Core`. It ends when the store drops.
pub(crate) struct Store {
    jobs: mpsc::Sender<Msg>,
}

impl Store {
    pub(crate) fn spawn(mut core: Core) -> Result<Store, MeshError> {
        let (jobs, mut queue) = mpsc::channel::<Msg>(QUEUE);
        thread::Builder::new()
            .name("mesh-store".to_owned())
            .spawn(move || {
                while let Some(msg) = queue.blocking_recv() {
                    match msg {
                        Msg::Job(job) => job(&mut core),
                        Msg::Stop(done) => {
                            drop(core);
                            done.send(()).ok();
                            return;
                        }
                    }
                }
            })?;
        // The thread ends when the last sender drops. It is not joined: a
        // drop on an async worker must not wait for a disk. `close` waits.
        Ok(Store { jobs })
    }

    /// Runs `f` on the owner thread and returns its answer.
    pub(crate) async fn run<R: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Core) -> Result<R, MeshError> + Send + 'static,
    ) -> Result<R, MeshError> {
        let (tx, rx) = oneshot::channel();
        let job: Job = Box::new(move |core| {
            // The caller may have gone away; the work is already done.
            tx.send(f(core)).ok();
        });
        self.jobs
            .send(Msg::Job(job))
            .await
            .map_err(|_| MeshError::Closed)?;
        rx.await.map_err(|_| MeshError::Closed)?
    }

    /// Finishes the jobs queued before it, then stops the thread. Returns
    /// once the thread is done with the files.
    pub(crate) async fn close(&self) {
        let (done, stopped) = oneshot::channel();
        if self.jobs.send(Msg::Stop(done)).await.is_ok() {
            stopped.await.ok();
        }
    }
}
