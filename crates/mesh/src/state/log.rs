use super::{Entry, Key};
use crate::clock::now_ms;
use crate::{Devices, MeshError, WriteLimit};
use beep_proto::lenient;
use iroh::{EndpointId, SecretKey};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use tracing::warn;

/// The folder under a device's directory that holds its mesh. The postcard
/// meshes of earlier versions lived in `mesh`, which is never read.
pub(crate) const MESH_DIR: &str = "mesh2";
pub(crate) const OLD_MESH_DIR: &str = "mesh";
/// A record larger than this is taken as damage.
const MAX_RECORD: usize = 64 * 1024 * 1024;
/// An entry stamped further ahead than this is refused.
const MAX_AHEAD_MS: u64 = 24 * 60 * 60 * 1000;
const HEADER: usize = 8;

/// Origin to the highest sequence number held.
pub type Heads = BTreeMap<EndpointId, u64>;

/// What `apply` did. A refused entry never hides the ones that counted.
#[derive(Debug, Default)]
pub struct Applied {
    /// Keys whose value changed.
    pub changed: Vec<Key>,
    /// Entries turned away, and why.
    pub refused: Vec<MeshError>,
}

#[derive(Debug, Serialize, Deserialize)]
enum Record {
    Heads(#[serde(with = "crate::wire::heads")] Heads),
    Entry(Entry),
}

/// Where the winning entry of a key sits in `logs`.
#[derive(Clone, Copy, Debug)]
struct Slot {
    origin: EndpointId,
    seq: u64,
}

pub struct State {
    secret: SecretKey,
    mesh: EndpointId,
    file: File,
    /// Bytes of valid log in `file`.
    len: u64,
    heads: Heads,
    logs: BTreeMap<EndpointId, BTreeMap<u64, Entry>>,
    winners: BTreeMap<Key, Slot>,
    highest_ms: u64,
}

impl fmt::Debug for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("State")
            .field("mesh", &self.mesh)
            .field("heads", &self.heads)
            .finish_non_exhaustive()
    }
}

pub(crate) fn private_options() -> OpenOptions {
    #[cfg_attr(not(unix), allow(unused_mut))]
    let mut options = OpenOptions::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
}

pub(crate) fn private_dir(dir: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(dir)
}

fn encode(records: &[Record]) -> Result<Vec<u8>, MeshError> {
    let mut buf = Vec::new();

    for record in records {
        let body = rmp_serde::to_vec(record)?;
        let len = u32::try_from(body.len()).map_err(|_| MeshError::TooLarge)?;
        buf.extend(len.to_le_bytes());
        buf.extend(crc32fast::hash(&body).to_le_bytes());
        buf.extend(body);
    }

    Ok(buf)
}

/// The records before the first bad or short one, and the bytes they use.
fn decode(bytes: &[u8]) -> (Vec<Record>, usize) {
    let mut out = Vec::new();
    let mut at = 0;

    while let Some(head) = bytes.get(at..at + HEADER) {
        let len = u32::from_le_bytes([head[0], head[1], head[2], head[3]]) as usize;
        let crc = u32::from_le_bytes([head[4], head[5], head[6], head[7]]);

        if len > MAX_RECORD {
            break;
        }

        let Some(body) = bytes.get(at + HEADER..at + HEADER + len) else {
            break;
        };

        if crc32fast::hash(body) != crc {
            break;
        }

        let Ok(record) = lenient::from_slice(body) else {
            break;
        };
        out.push(record);
        at += HEADER + len;
    }
    (out, at)
}

pub(crate) fn sync_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    File::open(dir)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = dir;
    Ok(())
}

impl State {
    /// Opens `<dir>/mesh2/state.log` for the mesh `mesh`, as the device that
    /// holds `secret`, and creates the log if absent.
    pub fn open(dir: &Path, secret: &SecretKey, mesh: EndpointId) -> Result<State, MeshError> {
        let mesh_dir = dir.join(MESH_DIR);
        private_dir(&mesh_dir)?;
        let path = mesh_dir.join("state.log");
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e.into()),
        };
        // The file may predate the 0600 rule.
        #[cfg(unix)]
        if !bytes.is_empty() {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        }

        let (records, good) = decode(&bytes);
        let mut state = State {
            secret: secret.clone(),
            mesh,
            // Replaced below, once the log is settled.
            file: private_options().append(true).create(true).open(&path)?,
            len: good as u64,
            heads: Heads::new(),
            logs: BTreeMap::new(),
            winners: BTreeMap::new(),
            highest_ms: 0,
        };
        let mut held = 0usize;

        for record in records {
            match record {
                Record::Heads(heads) => {
                    for (origin, seq) in heads {
                        let head = state.heads.entry(origin).or_default();
                        *head = (*head).max(seq);
                    }
                }
                Record::Entry(entry) => {
                    held += 1;
                    state.insert(entry);
                }
            }
        }

        let superseded = held > state.winners.len();

        if good < bytes.len() {
            warn!(
                kept = good,
                dropped = bytes.len() - good,
                "state log has a damaged tail, cutting it"
            );
            state.file.set_len(good as u64)?;
            state.file.sync_all()?;
        }

        if superseded {
            state.compact(&path)?;
        }

        Ok(state)
    }

    /// Rewrites the log as the heads and the winning entries only.
    fn compact(&mut self, path: &Path) -> Result<(), MeshError> {
        let keep: BTreeSet<(EndpointId, u64)> =
            self.winners.values().map(|s| (s.origin, s.seq)).collect();
        for (origin, log) in &mut self.logs {
            log.retain(|seq, _| keep.contains(&(*origin, *seq)));
        }

        let mut records = vec![Record::Heads(self.heads.clone())];
        records.extend(
            self.logs
                .values()
                .flatten()
                .map(|(_, e)| Record::Entry(e.clone())),
        );
        let buf = encode(&records)?;
        let tmp = path.with_extension("tmp");
        let mut out = private_options()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp)?;
        out.write_all(&buf)?;
        out.sync_all()?;
        drop(out);
        fs::rename(&tmp, path)?;

        if let Some(dir) = path.parent() {
            sync_dir(dir)?;
        }

        self.file = private_options().append(true).open(path)?;
        self.len = buf.len() as u64;
        Ok(())
    }

    /// Writes and fsyncs. On failure the file goes back to its old length.
    fn append(&mut self, entries: &[Entry]) -> Result<(), MeshError> {
        let records: Vec<Record> = entries.iter().cloned().map(Record::Entry).collect();
        let buf = encode(&records)?;
        let written = self
            .file
            .write_all(&buf)
            .and_then(|()| self.file.sync_data());
        if let Err(e) = written {
            // Best effort: a short record is cut at the next open anyway.
            let _ = self.file.set_len(self.len);
            return Err(e.into());
        }

        self.len += buf.len() as u64;
        Ok(())
    }

    fn insert(&mut self, entry: Entry) {
        self.highest_ms = self.highest_ms.max(entry.at_ms);
        let head = self.heads.entry(entry.origin).or_default();
        *head = (*head).max(entry.seq);
        let slot = Slot {
            origin: entry.origin,
            seq: entry.seq,
        };
        let beats = self
            .winner(&entry.key)
            .is_none_or(|old| (entry.at_ms, entry.origin) > (old.at_ms, old.origin));
        if beats {
            self.winners.insert(entry.key.clone(), slot);
        }
        self.logs
            .entry(entry.origin)
            .or_default()
            .insert(entry.seq, entry);
    }

    /// Writes `key` as this device and returns the entry to send to peers.
    /// Call it on a desktop only.
    pub fn set(&mut self, key: Key, value: Option<Vec<u8>>) -> Result<Entry, MeshError> {
        let me = self.secret.public();
        let head = self.heads.get(&me).copied().unwrap_or(0);
        let seq = head.checked_add(1).ok_or(MeshError::Clock)?.max(now_ms());
        let at_ms = self
            .highest_ms
            .checked_add(1)
            .ok_or(MeshError::Clock)?
            .max(now_ms());
        let entry = Entry::sign(&self.secret, self.mesh, key, value, at_ms, seq)?;
        self.append(std::slice::from_ref(&entry))?;
        self.insert(entry.clone());
        Ok(entry)
    }

    fn winner(&self, key: &Key) -> Option<&Entry> {
        let slot = self.winners.get(key)?;
        self.logs.get(&slot.origin)?.get(&slot.seq)
    }

    pub fn get(&self, key: &Key) -> Option<&[u8]> {
        self.winner(key)?.value.as_deref()
    }

    /// Live keys whose text form starts with `prefix`, such as `project/`.
    pub fn scan(&self, prefix: &str) -> Vec<(Key, &[u8])> {
        self.winners
            .keys()
            .filter(|key| key.to_string().starts_with(prefix))
            .filter_map(|key| Some((key.clone(), self.get(key)?)))
            .collect()
    }

    /// Whether `key` has an entry, a deleted one included.
    pub fn holds(&self, key: &Key) -> bool {
        self.winners.contains_key(key)
    }

    pub fn heads(&self) -> Heads {
        self.heads.clone()
    }

    /// Everything this side holds above `theirs`, per origin by rising seq.
    pub fn since(&self, theirs: &Heads) -> Vec<Entry> {
        let mut out = Vec::new();

        for (origin, log) in &self.logs {
            let have = theirs.get(origin).copied().unwrap_or(0);
            out.extend(log.range(have.saturating_add(1)..).map(|(_, e)| e.clone()));
        }

        out
    }

    /// Why `entry` may not be taken, if it may not. Signature first.
    fn check(&self, entry: &Entry, devices: &Devices, now: u64) -> Result<(), MeshError> {
        entry.verify(self.mesh)?;

        match devices.write_limit(&entry.origin) {
            WriteLimit::Always => {}
            WriteLimit::Refused => return Err(MeshError::NotDesktop(entry.origin)),
            WriteLimit::Until(limit) if entry.at_ms <= limit => {}
            WriteLimit::Until(_) => return Err(MeshError::Removed(entry.origin)),
        }

        if entry.at_ms > now.saturating_add(MAX_AHEAD_MS) {
            return Err(MeshError::Future(entry.origin));
        }

        Ok(())
    }

    /// Takes entries from peers. A refused entry stops its origin for this
    /// batch (later numbers must not move the head past it); other origins
    /// go on. `Err` is for I/O only.
    pub fn apply(
        &mut self,
        mut entries: Vec<Entry>,
        devices: &Devices,
    ) -> Result<Applied, MeshError> {
        entries.sort_by_key(|e| (e.origin, e.seq));
        let now = now_ms();
        let mut applied = Applied::default();
        let mut stopped = BTreeSet::new();
        let mut skipped = Vec::new();
        let mut next = self.heads.clone();
        let mut fresh = Vec::new();

        for entry in entries {
            if stopped.contains(&entry.origin) {
                continue;
            }

            if let Err(e) = self.check(&entry, devices, now) {
                stopped.insert(entry.origin);
                applied.refused.push(e);
                continue;
            }

            let head = next.get(&entry.origin).copied().unwrap_or(0);

            if entry.seq <= head {
                continue;
            }

            next.insert(entry.origin, entry.seq);

            if entry.key.is_malformed() {
                // Signed by a desktop but not a key: nothing to keep, and the origin's later
                // entries still count, so the head moves past it.
                applied
                    .refused
                    .push(MeshError::BadKey(entry.key.to_string()));
                skipped.push((entry.origin, entry.seq));
                continue;
            }

            fresh.push(entry);
        }

        for (origin, seq) in skipped {
            let head = self.heads.entry(origin).or_insert(0);
            *head = (*head).max(seq);
        }

        if fresh.is_empty() {
            return Ok(applied);
        }

        let touched: BTreeSet<Key> = fresh.iter().map(|e| e.key.clone()).collect();
        let before: Vec<Option<Vec<u8>>> = touched
            .iter()
            .map(|key| self.get(key).map(<[u8]>::to_vec))
            .collect();
        self.append(&fresh)?;

        for entry in fresh {
            self.insert(entry);
        }

        applied.changed = touched
            .into_iter()
            .zip(before)
            .filter(|(key, old)| self.get(key) != old.as_deref())
            .map(|(key, _)| key)
            .collect();
        Ok(applied)
    }
}

#[cfg(test)]
#[path = "../tests/state.rs"]
mod tests;
