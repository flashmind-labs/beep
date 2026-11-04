//! Replicated last-write-wins state. One log per origin device, numbered
//! from one. On connect each side sends its heads and receives what it
//! lacks. The value of a key is the entry with the highest `(at_ms, origin)`.
//!
//! Kept in `<dir>/mesh/state.log`, mode 0600, one length-prefixed record per
//! entry. Compaction at start empties every entry that lost its key (value
//! `None`, `at_ms` 0, which loses to any real write) but keeps its number, so
//! each origin's log stays gapless for a peer that has nothing yet.

use crate::{Devices, MeshError};
use iroh::EndpointId;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

/// A record larger than this is taken as a damaged file.
const MAX_RECORD: usize = 64 * 1024 * 1024;

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

/// One write. `None` is a delete.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub key: Key,
    pub value: Option<Vec<u8>>,
    pub at_ms: u64,
    pub origin: EndpointId,
    pub seq: u64,
}

/// Origin to the highest sequence number held.
pub type Heads = BTreeMap<EndpointId, u64>;

/// Where the winning entry of a key sits in `logs`.
#[derive(Clone, Copy, Debug)]
struct Slot {
    origin: EndpointId,
    seq: u64,
}

pub struct State {
    me: EndpointId,
    file: File,
    /// Per origin, the entry numbered `n` is at index `n - 1`.
    logs: BTreeMap<EndpointId, Vec<Entry>>,
    winners: BTreeMap<Key, Slot>,
    highest_ms: u64,
}

fn now_ms() -> u64 {
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    u64::try_from(since).unwrap_or(u64::MAX)
}

fn private_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
}

fn private_dir(dir: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(dir)
}

fn encode(entries: &[&Entry]) -> Result<Vec<u8>, MeshError> {
    let mut buf = Vec::new();
    for entry in entries {
        let body = postcard::to_stdvec(entry)?;
        let len = u32::try_from(body.len()).map_err(|_| MeshError::Corrupt)?;
        buf.extend(len.to_le_bytes());
        buf.extend(body);
    }
    Ok(buf)
}

/// Records up to the first incomplete one, which a crash mid-append leaves.
fn decode(mut bytes: &[u8]) -> Result<Vec<Entry>, MeshError> {
    let mut out = Vec::new();
    while let Some((len, rest)) = bytes.split_first_chunk::<4>() {
        let len = usize::try_from(u32::from_le_bytes(*len)).map_err(|_| MeshError::Corrupt)?;
        if len > MAX_RECORD {
            return Err(MeshError::Corrupt);
        }
        let Some((body, rest)) = rest.split_at_checked(len) else {
            break;
        };
        out.push(postcard::from_bytes(body).map_err(|_| MeshError::Corrupt)?);
        bytes = rest;
    }
    Ok(out)
}

impl State {
    /// Opens `<dir>/mesh/state.log`, compacting it, and creates it if absent.
    pub fn open(dir: &Path, me: EndpointId) -> Result<State, MeshError> {
        let mesh_dir = dir.join("mesh");
        private_dir(&mesh_dir)?;
        let path = mesh_dir.join("state.log");
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e.into()),
        };
        let mut logs: BTreeMap<EndpointId, Vec<Entry>> = BTreeMap::new();
        let mut winners = BTreeMap::new();
        let mut highest_ms = 0;
        for entry in decode(&bytes)? {
            let log = logs.entry(entry.origin).or_default();
            let have = log.len() as u64;
            if entry.seq != have + 1 {
                return Err(MeshError::Corrupt);
            }
            highest_ms = highest_ms.max(entry.at_ms);
            log.push(entry);
        }
        for (origin, log) in &logs {
            for entry in log {
                Self::record_winner(&logs, &mut winners, entry, *origin);
            }
        }
        let file = Self::compact(&path, &mut logs, &winners)?;
        Ok(State {
            me,
            file,
            logs,
            winners,
            highest_ms,
        })
    }

    fn compact(
        path: &Path,
        logs: &mut BTreeMap<EndpointId, Vec<Entry>>,
        winners: &BTreeMap<Key, Slot>,
    ) -> Result<File, MeshError> {
        for (origin, log) in logs.iter_mut() {
            for entry in log.iter_mut() {
                let wins = winners
                    .get(&entry.key)
                    .is_some_and(|w| w.origin == *origin && w.seq == entry.seq);
                if !wins {
                    entry.value = None;
                    entry.at_ms = 0;
                }
            }
        }
        let all: Vec<&Entry> = logs.values().flatten().collect();
        let tmp = path.with_extension("tmp");
        let mut out = private_options()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp)?;
        out.write_all(&encode(&all)?)?;
        out.sync_all()?;
        drop(out);
        fs::rename(&tmp, path)?;
        // The file may predate the 0600 rule.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        }
        Ok(private_options().append(true).open(path)?)
    }

    /// Makes `entry` the winner of its key when it beats the current one.
    fn record_winner(
        logs: &BTreeMap<EndpointId, Vec<Entry>>,
        winners: &mut BTreeMap<Key, Slot>,
        entry: &Entry,
        origin: EndpointId,
    ) {
        let beats = winners.get(&entry.key).is_none_or(|slot| {
            let old = &logs[&slot.origin][(slot.seq - 1) as usize];
            (entry.at_ms, origin) > (old.at_ms, old.origin)
        });
        if beats {
            winners.insert(
                entry.key.clone(),
                Slot {
                    origin,
                    seq: entry.seq,
                },
            );
        }
    }

    fn append(&mut self, entries: &[&Entry]) -> Result<(), MeshError> {
        self.file.write_all(&encode(entries)?)?;
        self.file.sync_data()?;
        Ok(())
    }

    fn insert(&mut self, entry: Entry) {
        self.highest_ms = self.highest_ms.max(entry.at_ms);
        let origin = entry.origin;
        let log = self.logs.entry(origin).or_default();
        log.push(entry);
        // The entry just pushed is the newest of its log.
        let newest = log[log.len() - 1].clone();
        Self::record_winner(&self.logs, &mut self.winners, &newest, origin);
    }

    /// Writes `key` as this device and returns the entry to send to peers.
    /// Call it on a desktop only.
    pub fn set(&mut self, key: Key, value: Option<Vec<u8>>) -> Result<Entry, MeshError> {
        let seq = self.logs.get(&self.me).map_or(0, Vec::len) as u64 + 1;
        let entry = Entry {
            key,
            value,
            at_ms: now_ms().max(self.highest_ms.saturating_add(1)),
            origin: self.me,
            seq,
        };
        self.append(&[&entry])?;
        self.insert(entry.clone());
        Ok(entry)
    }

    fn winner(&self, key: &Key) -> Option<&Entry> {
        let slot = self.winners.get(key)?;
        Some(&self.logs[&slot.origin][(slot.seq - 1) as usize])
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
        self.logs
            .iter()
            .map(|(origin, log)| (*origin, log.len() as u64))
            .collect()
    }

    /// Everything this side holds that `theirs` does not.
    pub fn since(&self, theirs: &Heads) -> Vec<Entry> {
        let mut out = Vec::new();
        for (origin, log) in &self.logs {
            let have = theirs.get(origin).copied().unwrap_or(0);
            let skip = usize::try_from(have).unwrap_or(usize::MAX);
            out.extend(log.iter().skip(skip).cloned());
        }
        out
    }

    /// Takes entries from peers. All or nothing: a gap, or an origin that is
    /// not a desktop in `devices`, refuses the whole batch. Returns the keys
    /// whose value changed.
    pub fn apply(
        &mut self,
        mut entries: Vec<Entry>,
        devices: &Devices,
    ) -> Result<Vec<Key>, MeshError> {
        entries.sort_by_key(|e| (e.origin, e.seq));
        let mut next: Heads = self.heads();
        let mut fresh = Vec::new();
        for entry in entries {
            if !devices.is_desktop(entry.origin) {
                return Err(MeshError::NotDesktop(entry.origin));
            }
            let have = next.get(&entry.origin).copied().unwrap_or(0);
            if entry.seq <= have && entry.seq != 0 {
                continue;
            }
            if entry.seq != have + 1 {
                return Err(MeshError::Gap {
                    origin: entry.origin,
                    have,
                    got: entry.seq,
                });
            }
            next.insert(entry.origin, entry.seq);
            fresh.push(entry);
        }
        if fresh.is_empty() {
            return Ok(Vec::new());
        }
        let touched: BTreeSet<Key> = fresh.iter().map(|e| e.key.clone()).collect();
        let before: Vec<Option<Vec<u8>>> = touched
            .iter()
            .map(|key| self.get(key).map(<[u8]>::to_vec))
            .collect();
        self.append(&fresh.iter().collect::<Vec<_>>())?;
        for entry in fresh {
            self.insert(entry);
        }
        Ok(touched
            .into_iter()
            .zip(before)
            .filter(|(key, old)| self.get(key) != old.as_deref())
            .map(|(key, _)| key)
            .collect())
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
        let state = State::open(dir.path(), mesh.keys[who].public()).expect("open");
        (dir, state)
    }

    fn project(name: &str) -> Key {
        Key::Project(name.to_owned())
    }

    fn val(s: &str) -> Option<Vec<u8>> {
        Some(s.as_bytes().to_vec())
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
            .map(|i| Entry {
                key: project("p"),
                value: val(&format!("v{i}")),
                at_ms: 500,
                origin: m.keys[i].public(),
                seq: 1,
            })
            .collect();
        let winner = (0..3)
            .max_by_key(|i| m.keys[*i].public())
            .expect("three keys");
        for order in orders(&entries) {
            let (_dir, mut s) = open(&m, 0);
            for e in order {
                s.apply(vec![e], &m.devices).expect("apply");
            }
            assert_eq!(s.get(&project("p")), val(&format!("v{winner}")).as_deref());
        }
        // One batch in any order converges too.
        for order in orders(&entries) {
            let (_dir, mut s) = open(&m, 1);
            s.apply(order, &m.devices).expect("apply");
            assert_eq!(s.get(&project("p")), val(&format!("v{winner}")).as_deref());
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
        let changed = b.apply(for_b, &m.devices).expect("apply");
        assert_eq!(changed.len(), 2);
        let for_a = b.since(&a.heads());
        assert_eq!(for_a.len(), 1);
        a.apply(for_a, &m.devices).expect("apply");
        assert!(a.since(&b.heads()).is_empty());
        assert!(b.since(&a.heads()).is_empty());
        assert_eq!(a.heads(), b.heads());
        // A node reads everything and writes nothing.
        n.apply(a.since(&n.heads()), &m.devices).expect("apply");
        assert_eq!(n.get(&project("two")), val("2").as_deref());
        assert_eq!(n.get(&Key::Flsh), val("f").as_deref());
        assert_eq!(n.scan("project/").len(), 2);
        // Reapplying changes nothing.
        let again = n.apply(a.since(&Heads::new()), &m.devices).expect("apply");
        assert!(again.is_empty());
    }

    #[test]
    fn a_gap_in_an_origins_log_is_refused() {
        let m = mesh(2, 0);
        let (_da, mut a) = open(&m, 0);
        let (_db, mut b) = open(&m, 1);
        a.set(project("one"), val("1")).expect("set");
        a.set(project("two"), val("2")).expect("set");
        let mut lacking = a.since(&Heads::new());
        lacking.remove(0);
        let err = b.apply(lacking, &m.devices);
        assert!(matches!(err, Err(MeshError::Gap { .. })));
        assert!(b.heads().is_empty());
        assert_eq!(b.get(&project("two")), None);
    }

    #[test]
    fn an_entry_from_a_node_is_refused() {
        let m = mesh(1, 1);
        let (_da, mut a) = open(&m, 0);
        let node_entry = Entry {
            key: Key::Credential("c".into()),
            value: val("stolen"),
            at_ms: u64::MAX,
            origin: m.keys[1].public(),
            seq: 1,
        };
        let stranger = Entry {
            origin: SecretKey::generate().public(),
            ..node_entry.clone()
        };
        for entry in [node_entry, stranger] {
            let err = a.apply(vec![entry], &m.devices);
            assert!(matches!(err, Err(MeshError::NotDesktop(_))));
        }
        assert_eq!(a.get(&Key::Credential("c".into())), None);
        assert!(a.heads().is_empty());
    }

    #[test]
    fn state_reads_back_after_reopening() {
        let m = mesh(2, 0);
        let dir = TempDir::new().expect("tempdir");
        let me = m.keys[0].public();
        {
            let mut s = State::open(dir.path(), me).expect("open");
            s.set(project("a"), val("1")).expect("set");
            s.set(project("b"), val("2")).expect("set");
            s.set(project("a"), None).expect("set");
            let theirs = Entry {
                key: Key::Pin("x".into()),
                value: val("y"),
                at_ms: 9,
                origin: m.keys[1].public(),
                seq: 1,
            };
            s.apply(vec![theirs], &m.devices).expect("apply");
        }
        let mut s = State::open(dir.path(), me).expect("reopen");
        assert_eq!(s.get(&project("a")), None);
        assert_eq!(s.get(&project("b")), val("2").as_deref());
        assert_eq!(s.get(&Key::Pin("x".into())), val("y").as_deref());
        assert_eq!(s.heads().get(&me), Some(&3));
        let next = s.set(project("c"), val("3")).expect("set");
        assert_eq!(next.seq, 4);
    }

    #[test]
    fn compaction_keeps_the_heads() {
        let m = mesh(2, 0);
        let dir = TempDir::new().expect("tempdir");
        let me = m.keys[0].public();
        let heads;
        let big = vec![7u8; 4096];
        {
            let mut s = State::open(dir.path(), me).expect("open");
            for _ in 0..20 {
                s.set(project("a"), Some(big.clone())).expect("set");
            }
            s.set(project("b"), val("last")).expect("set");
            heads = s.heads();
        }
        let file = dir.path().join("mesh").join("state.log");
        let before = std::fs::metadata(&file).expect("log").len();
        let s = State::open(dir.path(), me).expect("reopen");
        let after = std::fs::metadata(&file).expect("log").len();
        assert!(after < before / 4, "{after} vs {before}");
        assert_eq!(s.heads(), heads);
        assert_eq!(s.get(&project("a")), Some(big.as_slice()));
        assert_eq!(s.get(&project("b")), val("last").as_deref());
        // A fresh peer still gets a log it can apply, without a gap.
        let (_d, mut fresh) = open(&m, 1);
        fresh
            .apply(s.since(&Heads::new()), &m.devices)
            .expect("compacted log applies");
        assert_eq!(fresh.get(&project("a")), Some(big.as_slice()));
        assert_eq!(fresh.heads(), heads);
    }

    #[test]
    fn the_clock_moves_past_what_it_has_seen() {
        let m = mesh(2, 0);
        let (_d, mut s) = open(&m, 0);
        let future = Entry {
            key: project("p"),
            value: val("theirs"),
            at_ms: u64::MAX / 2,
            origin: m.keys[1].public(),
            seq: 1,
        };
        s.apply(vec![future], &m.devices).expect("apply");
        let mine = s.set(project("p"), val("mine")).expect("set");
        assert_eq!(mine.at_ms, u64::MAX / 2 + 1);
        assert_eq!(s.get(&project("p")), val("mine").as_deref());
        let again = s.set(project("p"), val("again")).expect("set");
        assert!(again.at_ms > mine.at_ms);
    }

    #[cfg(unix)]
    #[test]
    fn the_file_is_only_readable_by_its_owner() {
        use std::os::unix::fs::PermissionsExt;
        let m = mesh(1, 0);
        let (dir, mut s) = open(&m, 0);
        s.set(Key::Flsh, val("x")).expect("set");
        let file = dir.path().join("mesh").join("state.log");
        let mode = |p: &Path| std::fs::metadata(p).expect("meta").permissions().mode() & 0o777;
        assert_eq!(mode(&file), 0o600);
        assert_eq!(mode(&dir.path().join("mesh")) & 0o077, 0);
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).expect("chmod");
        drop(s);
        State::open(dir.path(), m.keys[0].public()).expect("reopen");
        assert_eq!(mode(&file), 0o600);
    }
}
