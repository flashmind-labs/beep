//! Replicated last-write-wins state. One log per origin device. On connect
//! each side sends its heads and receives what it lacks. The value of a key
//! is the entry with the highest `(at_ms, origin)`.
//!
//! These calls block on fsync, so callers run them off the async workers.
//!
//! Every entry is signed by its origin over the mesh id, so a node relaying
//! entries cannot forge or alter one. Sequence numbers rise per origin but
//! are sparse: a new entry takes `max(head + 1, now_ms)`, so a desktop that
//! lost its log restarts above its old numbers, and compaction can delete
//! superseded entries. A receiver skips `seq <= head` and moves the head to
//! the entry's `seq`.
//!
//! Kept in `<dir>/mesh/state.log`, mode 0600. Each record is a length, a
//! CRC32 and a postcard body. Compaction at open rewrites the file as a
//! `Heads` record plus the newest entry per key. Two replicas' heads may
//! differ after compaction (one may have dropped its newest entry), so
//! callers use `since`, never a comparison of heads, to decide they are in
//! sync. The first bad or short record at open ends the log there.

use crate::{Devices, MeshError, WriteLimit};
use iroh::{EndpointId, SecretKey, Signature};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::warn;

const SIGN_DOMAIN: &[u8] = b"beep-mesh/1 state-entry";
/// A record larger than this is taken as damage.
const MAX_RECORD: usize = 64 * 1024 * 1024;
/// An entry stamped further ahead than this is refused.
const MAX_AHEAD_MS: u64 = 24 * 60 * 60 * 1000;
const HEADER: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Key {
    Project(String),
    Sync(String),
    Group(String),
    Pin(String),
    Credential(String),
    Harness(String),
    Flsh,
    Instructions,
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Key::Project(n) => write!(f, "project/{n}"),
            Key::Sync(n) => write!(f, "sync/{n}"),
            Key::Group(n) => write!(f, "group/{n}"),
            Key::Pin(n) => write!(f, "pin/{n}"),
            Key::Credential(n) => write!(f, "credential/{n}"),
            Key::Harness(n) => write!(f, "harness/{n}"),
            Key::Flsh => f.write_str("flsh"),
            Key::Instructions => f.write_str("instructions"),
        }
    }
}

impl FromStr for Key {
    type Err = MeshError;

    fn from_str(s: &str) -> Result<Self, MeshError> {
        let bad = || MeshError::BadKey(s.to_owned());
        match s {
            "flsh" => return Ok(Key::Flsh),
            "instructions" => return Ok(Key::Instructions),
            _ => {}
        }
        let (kind, name) = s.split_once('/').ok_or_else(bad)?;
        if name.is_empty() {
            return Err(bad());
        }
        let name = name.to_owned();
        match kind {
            "project" => Ok(Key::Project(name)),
            "sync" => Ok(Key::Sync(name)),
            "group" => Ok(Key::Group(name)),
            "pin" => Ok(Key::Pin(name)),
            "credential" => Ok(Key::Credential(name)),
            "harness" => Ok(Key::Harness(name)),
            _ => Err(bad()),
        }
    }
}

impl Serialize for Key {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Key {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d)?.parse().map_err(D::Error::custom)
    }
}

/// One signed write. `None` is a delete.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub key: Key,
    pub value: Option<Vec<u8>>,
    pub at_ms: u64,
    pub origin: EndpointId,
    pub seq: u64,
    sig: Signature,
}

impl Entry {
    fn message(
        mesh: EndpointId,
        key: &Key,
        value: &Option<Vec<u8>>,
        at_ms: u64,
        origin: EndpointId,
        seq: u64,
    ) -> Result<Vec<u8>, MeshError> {
        let mut msg = SIGN_DOMAIN.to_vec();
        msg.extend_from_slice(mesh.as_bytes());
        msg.extend(postcard::to_stdvec(&(key, value, at_ms, origin, seq))?);
        Ok(msg)
    }

    pub(crate) fn sign(
        secret: &SecretKey,
        mesh: EndpointId,
        key: Key,
        value: Option<Vec<u8>>,
        at_ms: u64,
        seq: u64,
    ) -> Result<Entry, MeshError> {
        let origin = secret.public();
        let sig = secret.sign(&Self::message(mesh, &key, &value, at_ms, origin, seq)?);
        Ok(Entry {
            key,
            value,
            at_ms,
            origin,
            seq,
            sig,
        })
    }

    fn verify(&self, mesh: EndpointId) -> Result<(), MeshError> {
        let msg = Self::message(
            mesh,
            &self.key,
            &self.value,
            self.at_ms,
            self.origin,
            self.seq,
        )?;
        self.origin
            .verify(&msg, &self.sig)
            .map_err(|_| MeshError::BadSignature)
    }
}

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
    Heads(Heads),
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

pub(crate) fn now_ms() -> u64 {
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    u64::try_from(since).unwrap_or(u64::MAX)
}

pub(crate) fn private_options() -> OpenOptions {
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
        let body = postcard::to_stdvec(record)?;
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
        let Ok(record) = postcard::from_bytes(body) else {
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
    /// Opens `<dir>/mesh/state.log` for the mesh `mesh`, as the device that
    /// holds `secret`, and creates the log if absent.
    pub fn open(dir: &Path, secret: &SecretKey, mesh: EndpointId) -> Result<State, MeshError> {
        let mesh_dir = dir.join("mesh");
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
            fresh.push(entry);
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
mod tests {
    use super::*;
    use crate::{Device, DeviceKind};
    use iroh::SecretKey;
    use tempfile::TempDir;

    struct Mesh {
        keys: Vec<SecretKey>,
        devices: Devices,
    }

    /// Keys 0..desktops are desktops, the rest are nodes.
    fn mesh(desktops: usize, nodes: usize) -> Mesh {
        let keys: Vec<SecretKey> = (0..desktops + nodes)
            .map(|_| SecretKey::generate())
            .collect();
        let mut devices = Devices::genesis(&keys[0], "d0", DeviceKind::Desktop);
        for (i, key) in keys.iter().enumerate().skip(1) {
            let kind = if i < desktops {
                DeviceKind::Desktop
            } else {
                DeviceKind::Node
            };
            let device = Device {
                id: key.public(),
                name: format!("d{i}"),
                kind,
                added_at_ms: 1,
            };
            devices.add(&keys[0], device).expect("member desktop adds");
        }
        Mesh { keys, devices }
    }

    fn open(mesh: &Mesh, who: usize) -> (TempDir, State) {
        let dir = TempDir::new().expect("tempdir");
        let state = State::open(dir.path(), &mesh.keys[who], mesh.devices.mesh()).expect("open");
        (dir, state)
    }

    fn reopen(mesh: &Mesh, who: usize, dir: &TempDir) -> State {
        State::open(dir.path(), &mesh.keys[who], mesh.devices.mesh()).expect("reopen")
    }

    fn entry(m: &Mesh, who: usize, key: Key, value: &str, at_ms: u64, seq: u64) -> Entry {
        Entry::sign(&m.keys[who], m.devices.mesh(), key, val(value), at_ms, seq).expect("signs")
    }

    fn project(name: &str) -> Key {
        Key::Project(name.to_owned())
    }

    fn val(s: &str) -> Option<Vec<u8>> {
        Some(s.as_bytes().to_vec())
    }

    fn log_path(dir: &TempDir) -> std::path::PathBuf {
        dir.path().join("mesh").join("state.log")
    }

    fn orders<T: Clone>(items: &[T]) -> Vec<Vec<T>> {
        if items.len() <= 1 {
            return vec![items.to_vec()];
        }
        let mut all = Vec::new();
        for i in 0..items.len() {
            let mut rest = items.to_vec();
            let first = rest.remove(i);
            for mut tail in orders(&rest) {
                tail.insert(0, first.clone());
                all.push(tail);
            }
        }
        all
    }

    #[test]
    fn keys_round_trip_through_text() {
        let keys = [
            project("a b"),
            Key::Sync("x".into()),
            Key::Group("7".into()),
            Key::Pin("p".into()),
            Key::Credential("c".into()),
            Key::Harness("claude".into()),
            Key::Flsh,
            Key::Instructions,
        ];
        for key in keys {
            assert_eq!(key.to_string().parse::<Key>().expect("parses"), key);
        }
        assert_eq!(project("a").to_string(), "project/a");
        assert!("project/".parse::<Key>().is_err());
        assert!("nope/x".parse::<Key>().is_err());
        assert!("flsh/x".parse::<Key>().is_err());
    }

    #[test]
    fn two_writes_at_one_millisecond_end_on_the_same_value_everywhere() {
        let m = mesh(3, 0);
        let entries: Vec<Entry> = (0..3)
            .map(|i| entry(&m, i, project("p"), &format!("v{i}"), 500, 1))
            .collect();
        let winner = (0..3)
            .max_by_key(|i| m.keys[*i].public())
            .expect("three keys");
        let want = val(&format!("v{winner}"));
        for order in orders(&entries) {
            let (_dir, mut s) = open(&m, 0);
            for e in order {
                s.apply(vec![e], &m.devices).expect("apply");
            }
            assert_eq!(s.get(&project("p")), want.as_deref());
        }
        for order in orders(&entries) {
            let (_dir, mut s) = open(&m, 1);
            let applied = s.apply(order, &m.devices).expect("apply");
            assert!(applied.refused.is_empty());
            assert_eq!(s.get(&project("p")), want.as_deref());
        }
    }

    #[test]
    fn what_a_peer_lacks_is_exactly_what_it_gets() {
        let m = mesh(2, 1);
        let (_da, mut a) = open(&m, 0);
        let (_db, mut b) = open(&m, 1);
        let (_dn, mut n) = open(&m, 2);
        a.set(project("one"), val("1")).expect("set");
        a.set(project("two"), val("2")).expect("set");
        b.set(Key::Flsh, val("f")).expect("set");
        let for_b = a.since(&b.heads());
        assert_eq!(for_b.len(), 2);
        assert!(for_b.iter().all(|e| e.origin == m.keys[0].public()));
        assert!(for_b[0].seq < for_b[1].seq);
        let applied = b.apply(for_b, &m.devices).expect("apply");
        assert_eq!(applied.changed.len(), 2);
        let for_a = b.since(&a.heads());
        assert_eq!(for_a.len(), 1);
        a.apply(for_a, &m.devices).expect("apply");
        assert!(a.since(&b.heads()).is_empty());
        assert!(b.since(&a.heads()).is_empty());
        assert_eq!(a.heads(), b.heads());
        n.apply(a.since(&n.heads()), &m.devices).expect("apply");
        assert_eq!(n.get(&project("two")), val("2").as_deref());
        assert_eq!(n.get(&Key::Flsh), val("f").as_deref());
        assert_eq!(n.scan("project/").len(), 2);
        let again = n.apply(a.since(&Heads::new()), &m.devices).expect("apply");
        assert!(again.changed.is_empty() && again.refused.is_empty());
    }

    #[test]
    fn a_compacted_log_still_converges_on_a_fresh_peer() {
        let m = mesh(2, 0);
        let dir = TempDir::new().expect("tempdir");
        {
            let mut s = reopen(&m, 0, &dir);
            for i in 0..10 {
                s.set(project("a"), val(&format!("a{i}"))).expect("set");
                s.set(project("b"), val(&format!("b{i}"))).expect("set");
            }
            s.set(project("a"), None).expect("set");
        }
        let s = reopen(&m, 0, &dir);
        let (_d, mut fresh) = open(&m, 1);
        let applied = fresh
            .apply(s.since(&Heads::new()), &m.devices)
            .expect("apply");
        assert!(applied.refused.is_empty());
        assert_eq!(fresh.get(&project("a")), None);
        assert_eq!(fresh.get(&project("b")), val("b9").as_deref());
        assert_eq!(fresh.scan("project/").len(), 1);
        assert!(s.since(&fresh.heads()).is_empty());
    }

    #[test]
    fn an_entry_from_a_node_is_refused() {
        let m = mesh(1, 1);
        let (_da, mut a) = open(&m, 0);
        let from_node = entry(&m, 1, Key::Credential("c".into()), "stolen", u64::MAX, 1);
        let stranger = SecretKey::generate();
        let from_stranger = Entry::sign(
            &stranger,
            m.devices.mesh(),
            Key::Credential("c".into()),
            val("x"),
            1,
            1,
        )
        .expect("signs");
        let applied = a
            .apply(vec![from_node, from_stranger], &m.devices)
            .expect("apply");
        assert_eq!(applied.refused.len(), 2);
        assert!(
            applied
                .refused
                .iter()
                .all(|e| matches!(e, MeshError::NotDesktop(_)))
        );
        assert!(applied.changed.is_empty());
        assert_eq!(a.get(&Key::Credential("c".into())), None);
        assert!(a.heads().is_empty());
    }

    #[test]
    fn an_entry_forged_by_a_relaying_node_is_refused() {
        let m = mesh(2, 1);
        let (_da, mut a) = open(&m, 0);
        let (_db, mut b) = open(&m, 1);
        // The node rewrites a real entry of desktop 0 and relays it to b.
        let mut forged = entry(&m, 0, Key::Credential("c".into()), "real", 10, 1);
        forged.value = val("forged");
        let applied = b.apply(vec![forged], &m.devices).expect("apply");
        assert!(matches!(applied.refused[..], [MeshError::BadSignature]));
        assert_eq!(b.get(&Key::Credential("c".into())), None);
        assert!(b.heads().is_empty());
        // The real entry with that seq still lands.
        let real = a
            .set(Key::Credential("c".into()), val("real"))
            .expect("set");
        let applied = b.apply(vec![real], &m.devices).expect("apply");
        assert_eq!(applied.changed, vec![Key::Credential("c".into())]);
        assert_eq!(b.get(&Key::Credential("c".into())), val("real").as_deref());
    }

    #[test]
    fn one_origins_bad_entry_does_not_block_another() {
        let m = mesh(3, 0);
        let (_d, mut s) = open(&m, 2);
        let mut bad = entry(&m, 0, project("x"), "1", 5, 1);
        bad.at_ms = 6;
        let later = entry(&m, 0, project("y"), "2", 5, 2);
        let good = entry(&m, 1, project("z"), "3", 5, 1);
        let applied = s.apply(vec![bad, later, good], &m.devices).expect("apply");
        assert_eq!(applied.refused.len(), 1);
        assert_eq!(applied.changed, vec![project("z")]);
        assert_eq!(s.get(&project("y")), None);
        assert_eq!(s.heads().get(&m.keys[0].public()), None);
        assert_eq!(s.heads().get(&m.keys[1].public()), Some(&1));
    }

    #[test]
    fn state_reads_back_after_reopening() {
        let m = mesh(2, 0);
        let dir = TempDir::new().expect("tempdir");
        let me = m.keys[0].public();
        {
            let mut s = reopen(&m, 0, &dir);
            s.set(project("a"), val("1")).expect("set");
            s.set(project("b"), val("2")).expect("set");
            s.set(project("a"), None).expect("set");
            let theirs = entry(&m, 1, Key::Pin("x".into()), "y", 9, 1);
            s.apply(vec![theirs], &m.devices).expect("apply");
        }
        let mut s = reopen(&m, 0, &dir);
        assert_eq!(s.get(&project("a")), None);
        assert_eq!(s.get(&project("b")), val("2").as_deref());
        assert_eq!(s.get(&Key::Pin("x".into())), val("y").as_deref());
        let head = s.heads()[&me];
        let next = s.set(project("c"), val("3")).expect("set");
        assert!(next.seq > head);
    }

    #[test]
    fn compaction_keeps_the_heads() {
        let m = mesh(2, 0);
        let dir = TempDir::new().expect("tempdir");
        let heads;
        let big = vec![7u8; 4096];
        {
            let mut s = reopen(&m, 0, &dir);
            for _ in 0..20 {
                s.set(project("a"), Some(big.clone())).expect("set");
            }
            // The head entry is itself superseded.
            s.set(project("b"), val("last")).expect("set");
            s.set(project("b"), val("last2")).expect("set");
            heads = s.heads();
        }
        let before = std::fs::metadata(log_path(&dir)).expect("log").len();
        let s = reopen(&m, 0, &dir);
        let after = std::fs::metadata(log_path(&dir)).expect("log").len();
        assert!(after < before / 4, "{after} vs {before}");
        assert_eq!(s.heads(), heads);
        assert_eq!(s.get(&project("a")), Some(big.as_slice()));
        assert_eq!(s.get(&project("b")), val("last2").as_deref());
        drop(s);
        // A second open has nothing to compact and keeps the heads.
        assert_eq!(reopen(&m, 0, &dir).heads(), heads);
    }

    #[test]
    fn the_clock_moves_past_what_it_has_seen() {
        let m = mesh(2, 0);
        let (_d, mut s) = open(&m, 0);
        let soon = now_ms() + 3_600_000;
        let theirs = entry(&m, 1, project("p"), "theirs", soon, 1);
        s.apply(vec![theirs], &m.devices).expect("apply");
        let mine = s.set(project("p"), val("mine")).expect("set");
        assert_eq!(mine.at_ms, soon + 1);
        assert_eq!(s.get(&project("p")), val("mine").as_deref());
        let again = s.set(project("p"), val("again")).expect("set");
        assert!(again.at_ms > mine.at_ms);
    }

    #[test]
    fn a_clock_that_cannot_move_on_is_an_error() {
        let m = mesh(1, 0);
        let (_d, mut s) = open(&m, 0);
        s.highest_ms = u64::MAX;
        assert!(matches!(
            s.set(project("p"), val("x")),
            Err(MeshError::Clock)
        ));
        assert_eq!(s.get(&project("p")), None);
    }

    #[test]
    fn an_entry_far_in_the_future_is_refused() {
        let m = mesh(2, 0);
        let (_d, mut s) = open(&m, 0);
        let day = 24 * 3_600_000;
        let far = entry(&m, 1, project("p"), "x", now_ms() + day + 60_000, 1);
        let applied = s.apply(vec![far], &m.devices).expect("apply");
        assert!(matches!(applied.refused[..], [MeshError::Future(_)]));
        assert_eq!(s.get(&project("p")), None);
        let ok = entry(&m, 1, project("p"), "x", now_ms() + day - 60_000, 1);
        let applied = s.apply(vec![ok], &m.devices).expect("apply");
        assert_eq!(applied.changed, vec![project("p")]);
    }

    #[test]
    fn a_desktop_that_lost_its_log_is_still_heard() {
        let m = mesh(2, 0);
        let (_db, mut b) = open(&m, 1);
        let old = entry(&m, 0, project("p"), "old", 5, 7);
        b.apply(vec![old], &m.devices).expect("apply");
        // Desktop 0 comes back with an empty directory.
        let (_da, mut a) = open(&m, 0);
        let fresh = a.set(project("p"), val("new")).expect("set");
        assert!(fresh.seq > 7);
        let applied = b.apply(vec![fresh], &m.devices).expect("apply");
        assert_eq!(applied.changed, vec![project("p")]);
        assert_eq!(b.get(&project("p")), val("new").as_deref());
    }

    #[test]
    fn a_removed_desktop_keeps_its_old_writes_and_loses_new_ones() {
        let mut m = mesh(2, 0);
        let gone = m.keys[1].public();
        let before = entry(&m, 1, project("old"), "kept", 5, 1);
        let removal = m.devices.remove(&m.keys[0], gone).expect("remove");
        assert_eq!(
            m.devices.write_limit(&gone),
            WriteLimit::Until(removal_ms(&m, gone))
        );
        let after = entry(&m, 1, project("new"), "late", now_ms() + 5_000, 2);
        let (_d, mut fresh) = open(&m, 0);
        let applied = fresh.apply(vec![before, after], &m.devices).expect("apply");
        assert_eq!(applied.changed, vec![project("old")]);
        assert!(matches!(applied.refused[..], [MeshError::Removed(_)]));
        assert_eq!(fresh.get(&project("new")), None);
        drop(removal);
    }

    fn removal_ms(m: &Mesh, id: EndpointId) -> u64 {
        match m.devices.write_limit(&id) {
            WriteLimit::Until(t) => t,
            other => panic!("not removed: {other:?}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn the_file_is_only_readable_by_its_owner() {
        use std::os::unix::fs::PermissionsExt;
        let m = mesh(1, 0);
        let (dir, mut s) = open(&m, 0);
        s.set(Key::Flsh, val("x")).expect("set");
        let file = log_path(&dir);
        let mode = |p: &Path| std::fs::metadata(p).expect("meta").permissions().mode() & 0o777;
        assert_eq!(mode(&file), 0o600);
        assert_eq!(mode(&dir.path().join("mesh")) & 0o077, 0);
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).expect("chmod");
        drop(s);
        reopen(&m, 0, &dir);
        assert_eq!(mode(&file), 0o600);
    }

    #[test]
    fn a_damaged_tail_is_cut_and_the_earlier_entries_survive() {
        use std::io::Write;
        let m = mesh(1, 0);
        for tail in [
            vec![0u8; 300],
            b"garbage that is no record".to_vec(),
            vec![9, 0],
        ] {
            let dir = TempDir::new().expect("tempdir");
            {
                let mut s = reopen(&m, 0, &dir);
                s.set(project("a"), val("1")).expect("set");
                s.set(project("b"), val("2")).expect("set");
            }
            OpenOptions::new()
                .append(true)
                .open(log_path(&dir))
                .expect("open")
                .write_all(&tail)
                .expect("write");
            let mut s = reopen(&m, 0, &dir);
            assert_eq!(s.get(&project("a")), val("1").as_deref());
            assert_eq!(s.get(&project("b")), val("2").as_deref());
            // New writes land after the cut and survive another open.
            s.set(project("c"), val("3")).expect("set");
            drop(s);
            let s = reopen(&m, 0, &dir);
            assert_eq!(s.get(&project("c")), val("3").as_deref());
            assert_eq!(s.get(&project("a")), val("1").as_deref());
        }
    }

    #[test]
    fn a_flipped_byte_in_a_record_cuts_the_log_there() {
        let m = mesh(1, 0);
        let dir = TempDir::new().expect("tempdir");
        {
            let mut s = reopen(&m, 0, &dir);
            s.set(project("a"), val("1")).expect("set");
            s.set(project("b"), val("2")).expect("set");
        }
        let mut bytes = std::fs::read(log_path(&dir)).expect("read");
        let last = bytes.len() - 1;
        bytes[last] ^= 0xff;
        std::fs::write(log_path(&dir), bytes).expect("write");
        let s = reopen(&m, 0, &dir);
        assert_eq!(s.get(&project("a")), val("1").as_deref());
        assert_eq!(s.get(&project("b")), None);
    }
}
