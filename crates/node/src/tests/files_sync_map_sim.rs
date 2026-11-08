//! A seeded simulator for `Map`, after TigerBeetle's VOPR: in-memory nodes
//! with a real `Map` each, making the calls `project/` and `peer.rs` make, on
//! a network that drops, reorders, duplicates and delays. One u64 decides it
//! all, so a run replays exactly.
//!
//! Replay a seed: `BEEP_SIM_SEED=7 cargo test -p beep-node --lib map_sim -- --nocapture`
//! Run more: `BEEP_SIM_SEEDS=1000000 cargo test --release -p beep-node --lib map_sim`

use super::{
    gitdir,
    map::{
        Entry, FileMeta, Map, Order, Replica, Version, conflict_path, is_conflict, original_of,
        valid_path,
    },
    project::Cursor,
};
use std::{
    collections::BTreeMap,
    env,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::Instant,
};

/// Seeds the default run covers, on top of `FAILED_SEEDS`.
const SEEDS: u64 = 10_000;
/// Seeds that once failed, which always run.
const FAILED_SEEDS: &[u64] = &[49, 117, 7950];
/// How many rounds of pulling nodes may need to settle once faults end.
const SETTLE_ROUNDS: usize = 16;

const PATHS: [&str; 6] = [
    "a.txt",
    "dir/b.txt",
    "dir/sub/c",
    "noext",
    ".hidden",
    "d.tar.gz",
];
/// A ref keeps its loser under another ref; `.git/HEAD` keeps no loser.
const GIT_PATHS: [&str; 2] = [".git/refs/heads/main", ".git/HEAD"];
const PAGES: [usize; 5] = [1, 2, 3, 8, 1000];

type Hash = [u8; 32];

fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// splitmix64.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        mix(self.0)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }

    fn shuffle<T>(&mut self, items: &mut [T]) {
        for at in (1..items.len()).rev() {
            items.swap(at, self.below(at + 1));
        }
    }
}

/// What one seed turns on.
#[derive(Debug)]
struct Config {
    nodes: usize,
    steps: usize,
    paths: Vec<&'static str>,
    /// The most entries one message carries.
    page: usize,
    deletes: bool,
    re_adds: bool,
    same_content: bool,
    equal_mtimes: bool,
    skew: bool,
    exec_bits: bool,
    edit_copies: bool,
    /// Every hash starts the same, so copies of different content share a name.
    tag_collisions: bool,
    /// Percent of messages lost, each breaking its stream.
    drop: u64,
    duplicate: u64,
    delay: u64,
    reorder: bool,
    partitions: bool,
    offline: bool,
    restarts: bool,
    /// Restarts from the last save, with what came after it gone.
    crashes: bool,
    lost_state: bool,
}

impl Config {
    fn new(rng: &mut Rng) -> Self {
        let mut paths: Vec<&str> = PATHS.to_vec();
        rng.shuffle(&mut paths);
        paths.truncate(1 + rng.below(PATHS.len()));
        if rng.chance(25) {
            paths.extend(GIT_PATHS);
        }
        let mut percent = |on: u64, of: u64| if rng.chance(on) { of } else { 0 };
        let (drop, duplicate, delay) = (percent(50, 20), percent(50, 20), percent(50, 40));
        let (crashes, lost_state) = (rng.chance(30), rng.chance(15));
        Self {
            nodes: 2 + rng.below(5),
            steps: 20 + rng.below(180),
            paths,
            page: PAGES[rng.below(PAGES.len())],
            deletes: rng.chance(80),
            re_adds: rng.chance(70),
            same_content: rng.chance(50),
            equal_mtimes: rng.chance(30),
            skew: rng.chance(50),
            exec_bits: rng.chance(30),
            edit_copies: rng.chance(30),
            tag_collisions: rng.chance(15),
            drop,
            duplicate,
            delay,
            // A stream cannot reorder, so a page never comes ahead of the
            // copy it depends on, whose only holder may then lose it.
            reorder: !crashes && !lost_state && rng.chance(50),
            partitions: rng.chance(50),
            offline: rng.chance(50),
            restarts: rng.chance(50),
            crashes,
            lost_state,
        }
    }
}

/// What a scan or a write left at a path, like `project::state::Seen`.
#[derive(Debug, Clone)]
struct Seen {
    hash: Hash,
    version: Version,
}

#[derive(Debug, Clone)]
struct Saved {
    map: Vec<u8>,
    seen: BTreeMap<String, Seen>,
    cursors: BTreeMap<usize, Cursor>,
}

struct Node {
    map: Map,
    seen: BTreeMap<String, Seen>,
    /// The last change had from each peer, by index.
    cursors: BTreeMap<usize, Cursor>,
    /// The folder, which outlives every restart.
    disk: BTreeMap<String, FileMeta>,
    saved: Option<Saved>,
    /// Crashes are left out where `Map` is known not to recover: after a load
    /// not saved since, as `Map::loaded` moves counts on
    /// in memory only, so a second crash hands them out again.
    unsaved_load: bool,
    /// A page that left a peer behind since the last save. An entry changed
    /// since the save has moved past that peer's cursor, and a crash puts it
    /// back before the cursor, so the peer never gets it.
    unsaved_partial_page: bool,
    /// How far this node's clock is off, in milliseconds.
    skew: i64,
}

impl Node {
    /// `move_target` and `Task::keep_as`: content about to be replaced moves
    /// to a path the list wants it at that it is not on disk at yet.
    fn keep_losing_side(&mut self, path: &str) {
        let Some(seen) = self.seen.get(path) else {
            return;
        };
        let target = self.map.entries().find(|entry| {
            entry.path != path
                && !gitdir::in_git(path)
                && !gitdir::in_git(&entry.path)
                && entry.file.as_ref().is_some_and(|f| f.hash == seen.hash)
                && !self.seen.contains_key(&entry.path)
        });
        let Some((to, version)) = target.map(|e| (e.path.clone(), e.version.clone())) else {
            return;
        };
        if let (Some(old), Some(file)) = (self.seen.remove(path), self.disk.remove(path)) {
            let hash = old.hash;
            self.disk.insert(to.clone(), file);
            self.seen.insert(to, Seen { hash, version });
        }
    }
}

/// One `Reply::Changes`.
#[derive(Debug, Clone)]
struct Message {
    from: usize,
    to: usize,
    replica: Replica,
    /// Where the stream stood before it.
    since: u64,
    upto: u64,
    entries: Vec<Entry>,
    /// The step it can arrive at.
    due: u64,
}

/// Content a node wrote and recorded.
struct Write {
    node: usize,
    path: String,
    hash: Hash,
    version: Version,
}

/// An edit or delete a node made: what it replaced, and the version it built on.
struct Overwrite {
    path: String,
    saw: Option<Hash>,
    base: Version,
}

/// What a run hit, to tell the seeds really reach the hard cases.
#[derive(Debug, Default, Clone, Copy)]
struct Tally {
    conflicts: u64,
    tombstones: u64,
    dropped: u64,
    restarts: u64,
    crashes: u64,
}

impl Tally {
    fn add(&mut self, other: Tally) {
        self.conflicts += other.conflicts;
        self.tombstones += other.tombstones;
        self.dropped += other.dropped;
        self.restarts += other.restarts;
        self.crashes += other.crashes;
    }
}

struct Sim {
    cfg: Config,
    rng: Rng,
    nodes: Vec<Node>,
    net: Vec<Message>,
    /// Open pulls by (client, server): how far the server has sent.
    streams: BTreeMap<(usize, usize), u64>,
    now: u64,
    down_until: Vec<u64>,
    group: Vec<usize>,
    split_until: u64,
    next_replica: Replica,
    next_content: u64,
    writes: Vec<Write>,
    overwrites: Vec<Overwrite>,
    /// Every entry `edit` made.
    authored: Vec<Entry>,
    trace: Option<Vec<String>>,
    /// Faults ended: the trace leaves out what each message carried.
    settling: bool,
    tally: Tally,
}

fn tag(hash: &Hash) -> String {
    hash[..4].iter().map(|b| format!("{b:02x}")).collect()
}

fn show(file: Option<&FileMeta>) -> String {
    file.map_or("deleted".to_owned(), |f| {
        let exec = if f.executable { " exec" } else { "" };
        format!("{} mtime {}{exec}", tag(&f.hash), f.mtime_ms)
    })
}

fn line(entry: &Entry) -> String {
    let file = show(entry.file.as_ref());
    format!("{} {:?} {file}", entry.path, entry.version)
}

/// The path a copy goes back to, or `.git` for all of it, whose refs name
/// their copies another way.
fn family(path: &str) -> String {
    if gitdir::in_git(path) {
        return ".git".to_owned();
    }
    let mut root = path.to_owned();
    while let Some(original) = original_of(&root) {
        root = original;
    }
    root
}

/// How many losing edits a path's name stacks up.
fn depth(path: &str) -> usize {
    if gitdir::in_git(path) {
        path.replace("beep-conflicts", "").matches('-').count()
    } else {
        path.matches(" (conflict ").count()
    }
}

/// The first path two lists hold differently.
fn difference(a: &Map, b: &Map) -> Option<String> {
    let path = a
        .entries()
        .chain(b.entries())
        .map(|entry| &entry.path)
        .find(|path| a.get(path) != b.get(path))?;
    let held = |map: &Map| map.get(path).map_or("nothing".to_owned(), line);
    Some(format!("{} and {}", held(a), held(b)))
}

fn has_seen(version: &Version, other: &Version) -> bool {
    matches!(version.compare(other), Order::Newer | Order::Same)
}

impl Sim {
    fn new(seed: u64, trace: bool) -> Self {
        let mut rng = Rng(seed);
        let cfg = Config::new(&mut rng);
        let nodes = (1..=cfg.nodes)
            .map(|at| Node {
                map: Map::new(at as Replica),
                seen: BTreeMap::new(),
                cursors: BTreeMap::new(),
                disk: BTreeMap::new(),
                saved: None,
                unsaved_load: false,
                unsaved_partial_page: false,
                skew: if cfg.skew {
                    // Up to some 17 years either way.
                    (rng.next() as i64) >> 24
                } else {
                    0
                },
            })
            .collect();
        Self {
            down_until: vec![0; cfg.nodes],
            group: vec![0; cfg.nodes],
            next_replica: cfg.nodes as Replica + 1,
            cfg,
            rng,
            nodes,
            net: Vec::new(),
            streams: BTreeMap::new(),
            now: 0,
            split_until: 0,
            next_content: 100,
            writes: Vec::new(),
            overwrites: Vec::new(),
            authored: Vec::new(),
            trace: trace.then(Vec::new),
            settling: false,
            tally: Tally::default(),
        }
    }

    fn note(&mut self, line: impl FnOnce() -> String) {
        if let Some(trace) = &mut self.trace {
            trace.push(format!("t{} {}", self.now, line()));
        }
    }

    fn run(&mut self) -> Result<(), String> {
        for _ in 0..self.cfg.steps {
            self.step()?;
        }
        self.settle()?;
        self.check()
    }

    fn step(&mut self) -> Result<(), String> {
        self.now += 1;
        let node = self.rng.below(self.cfg.nodes);
        match self.rng.below(100) {
            0..=24 => self.user_op(node),
            25..=54 => {
                let server = self.rng.below(self.cfg.nodes);
                if server != node {
                    self.pull(node, server)?;
                }
            }
            55..=84 => self.deliver_one()?,
            85..=91 => self.apply(node, false),
            92..=94 => self.save(node)?,
            95..=97 => self.network_fault(node),
            _ => self.restart(node)?,
        }
        Ok(())
    }

    /// One thing a person does in a node's folder, seen by the next scan.
    fn user_op(&mut self, n: usize) {
        let path = self.pick_path(n);
        let on_disk = self.nodes[n].disk.contains_key(&path);
        if on_disk
            && self.cfg.deletes
            && !gitdir::is_kept_from_deletes(&path)
            && self.rng.chance(30)
        {
            self.nodes[n].disk.remove(&path);
            self.note(|| format!("n{n} deletes {path}"));
            self.scan_delete(n, &path);
            return;
        }
        let tombstone = self.nodes[n]
            .map
            .get(&path)
            .is_some_and(|entry| entry.file.is_none());
        if !on_disk && tombstone && !self.cfg.re_adds {
            return;
        }
        let meta = self.new_content(n);
        self.note(|| format!("n{n} writes {path}: {}", show(Some(&meta))));
        self.nodes[n].disk.insert(path.clone(), meta.clone());
        self.record(n, &path, meta);
    }

    fn pick_path(&mut self, n: usize) -> String {
        if self.cfg.edit_copies && self.rng.chance(25) {
            let copies: Vec<&String> = self.nodes[n]
                .disk
                .keys()
                .filter(|path| is_conflict(path) && !gitdir::in_git(path))
                .collect();
            if !copies.is_empty() {
                return copies[self.rng.below(copies.len())].clone();
            }
        }
        self.cfg.paths[self.rng.below(self.cfg.paths.len())].to_owned()
    }

    fn new_content(&mut self, n: usize) -> FileMeta {
        let id = if self.cfg.same_content && self.rng.chance(40) {
            self.rng.below(4) as u64
        } else {
            self.next_content += 1;
            self.next_content
        };
        let mut hash = [0; 32];
        for (at, chunk) in hash.chunks_mut(8).enumerate() {
            chunk.copy_from_slice(&mix(id.wrapping_add(at as u64 + 1)).to_le_bytes());
        }
        if self.cfg.tag_collisions {
            hash[..4].fill(0xab);
        }
        let mtime_ms = if self.cfg.equal_mtimes {
            1000
        } else {
            self.now as i64 * 1000 + self.nodes[n].skew
        };
        FileMeta {
            hash,
            size: id,
            mtime_ms,
            executable: self.cfg.exec_bits && self.rng.chance(50),
        }
    }

    /// `Map::edit`, with what the checks need to know about it.
    fn edit(
        &mut self,
        n: usize,
        path: &str,
        base: &Version,
        file: Option<FileMeta>,
        saw: Option<Hash>,
    ) -> Version {
        let version = self.nodes[n].map.edit(path, base, file.clone());
        if let Some(file) = &file {
            self.writes.push(Write {
                node: n,
                path: path.to_owned(),
                hash: file.hash,
                version: version.clone(),
            });
        }
        self.overwrites.push(Overwrite {
            path: path.to_owned(),
            saw,
            base: base.clone(),
        });
        self.authored.push(Entry {
            path: path.to_owned(),
            version: version.clone(),
            file,
        });
        version
    }

    /// `project::list::record`: the scan found this content at `path`.
    fn record(&mut self, n: usize, path: &str, meta: FileMeta) {
        let hash = meta.hash;
        let listed = self.nodes[n].map.get(path).cloned();
        let same = |entry: &Entry, hash: Hash| entry.file.as_ref().is_some_and(|f| f.hash == hash);
        let version = match self.nodes[n].seen.get(path).cloned() {
            Some(prior) if prior.hash == hash => prior.version,
            Some(prior) => {
                let base = match listed {
                    Some(entry)
                        if same(&entry, prior.hash)
                            && entry.version.compare(&prior.version) == Order::Newer =>
                    {
                        entry.version
                    }
                    _ => prior.version,
                };
                self.edit(n, path, &base, Some(meta), Some(prior.hash))
            }
            None => match listed {
                Some(entry) if same(&entry, hash) => entry.version,
                _ => self.edit(n, path, &Version::default(), Some(meta), None),
            },
        };
        self.nodes[n]
            .seen
            .insert(path.to_owned(), Seen { hash, version });
    }

    /// `Task::try_send_deletes`: the scan found the file at `path` gone.
    fn scan_delete(&mut self, n: usize, path: &str) {
        if let Some(old) = self.nodes[n].seen.remove(path) {
            self.edit(n, path, &old.version, None, Some(old.hash));
        }
    }

    /// `Task::try_scan` over the whole folder, as after a start.
    fn scan(&mut self, n: usize) {
        let found: Vec<(String, FileMeta)> = self.nodes[n]
            .disk
            .iter()
            .map(|(path, meta)| (path.clone(), meta.clone()))
            .collect();
        for (path, meta) in found {
            if self.nodes[n].seen.get(&path).map(|seen| seen.hash) != Some(meta.hash) {
                // A node that lost state records its files as new writes,
                // which stand for the older ones of the same content.
                let family_of = family(&path);
                self.writes
                    .retain(|write| write.hash != meta.hash || family(&write.path) != family_of);
                self.record(n, &path, meta);
            }
        }
        let gone: Vec<String> = self.nodes[n]
            .seen
            .keys()
            .filter(|path| !self.nodes[n].disk.contains_key(*path))
            .cloned()
            .collect();
        for path in gone {
            self.scan_delete(n, &path);
        }
    }

    /// `plan` and `bring`: the folder follows the list, for a part of it
    /// unless `all`, as when some fetches fail.
    fn apply(&mut self, n: usize, all: bool) {
        let entries: Vec<Entry> = self.nodes[n].map.entries().cloned().collect();
        for entry in entries {
            if !all && self.rng.chance(20) {
                continue;
            }
            let path = &entry.path;
            let node = &mut self.nodes[n];
            let seen = node.seen.get(path).map(|seen| seen.hash);
            let wanted = entry.file.as_ref().map(|file| file.hash);
            if seen == wanted || (wanted.is_none() && gitdir::is_kept_from_deletes(path)) {
                continue;
            }
            node.keep_losing_side(path);
            match &entry.file {
                Some(file) => {
                    node.disk.insert(path.clone(), file.clone());
                    let version = entry.version.clone();
                    let hash = file.hash;
                    node.seen.insert(path.clone(), Seen { hash, version });
                }
                None => {
                    node.disk.remove(path);
                    node.seen.remove(path);
                }
            }
        }
    }

    fn connected(&self, a: usize, b: usize) -> bool {
        self.now >= self.down_until[a]
            && self.now >= self.down_until[b]
            && (self.now >= self.split_until || self.group[a] == self.group[b])
    }

    /// The next page of the server's changes on the client's open pull, or
    /// from the client's cursor on a new one, as in `peer::serve`.
    fn page(&mut self, client: usize, server: usize) -> Result<Option<Message>, String> {
        let replica = self.nodes[server].map.replica();
        let cursor = self.nodes[client]
            .cursors
            .get(&server)
            .copied()
            .unwrap_or_default();
        let since = *self.streams.entry((client, server)).or_insert({
            if cursor.replica == replica {
                cursor.seq
            } else {
                0
            }
        });
        let changes = self.nodes[server].map.since(since, self.cfg.page);
        if changes.entries.is_empty() {
            return Ok(None);
        }
        if changes.entries.len() > self.cfg.page || changes.upto <= since {
            return Err(format!(
                "since({since}, {}) gave {} entries up to {}",
                self.cfg.page,
                changes.entries.len(),
                changes.upto
            ));
        }
        self.streams.insert((client, server), changes.upto);
        self.nodes[server].unsaved_partial_page |= changes.upto < self.nodes[server].map.seq();
        Ok(Some(Message {
            from: server,
            to: client,
            replica,
            since,
            upto: changes.upto,
            entries: changes.entries,
            due: self.now,
        }))
    }

    fn pull(&mut self, client: usize, server: usize) -> Result<(), String> {
        if !self.connected(client, server) {
            self.streams.remove(&(client, server));
            return Ok(());
        }
        let Some(mut message) = self.page(client, server)? else {
            return Ok(());
        };
        self.note(|| {
            format!(
                "n{server} sends n{client} {}..{}: {} entries",
                message.since,
                message.upto,
                message.entries.len()
            )
        });
        // A stream delivers in order or breaks: the client pulls again from
        // its cursor, which never passed what was lost.
        if self.rng.chance(self.cfg.drop) {
            self.note(|| format!("dropped, stream n{client} from n{server} breaks"));
            self.streams.remove(&(client, server));
            self.tally.dropped += 1;
            return Ok(());
        }
        if self.rng.chance(self.cfg.delay) {
            message.due += 1 + self.rng.below(40) as u64;
        }
        if self.rng.chance(self.cfg.duplicate) {
            self.net.push(message.clone());
        }
        self.net.push(message);
        Ok(())
    }

    fn deliver_one(&mut self) -> Result<(), String> {
        let now = self.now;
        let due: Vec<usize> = (0..self.net.len())
            .filter(|&at| self.net[at].due <= now)
            .collect();
        let at = if self.cfg.reorder && !due.is_empty() {
            due[self.rng.below(due.len())]
        } else if due.first() == Some(&0) {
            0
        } else {
            return Ok(());
        };
        let message = self.net.remove(at);
        self.receive(&message)
    }

    /// `Shared::merge_from`. The real one sets the cursor to `upto` because a
    /// stream cannot skip; here messages can, so it only moves over what
    /// joins up. In order the two are the same.
    fn receive(&mut self, message: &Message) -> Result<(), String> {
        let (to, from) = (message.to, message.from);
        if !self.connected(to, from) {
            // The stream breaks, and all that followed on it goes too.
            self.note(|| format!("n{to} misses n{from} {}..", message.since));
            self.streams.remove(&(to, from));
            self.net.retain(|m| m.to != to || m.from != from);
            return Ok(());
        }
        let settling = self.settling;
        self.note(|| {
            if settling {
                return format!("n{to} gets n{from} {}..{}", message.since, message.upto);
            }
            let entries: Vec<String> = message.entries.iter().map(line).collect();
            format!(
                "n{to} gets n{from} {}..{}: {}",
                message.since,
                message.upto,
                entries.join(", ")
            )
        });
        for entry in &message.entries {
            if !valid_path(&entry.path) {
                return Err(format!("a peer would refuse {}", entry.path));
            }
            let map = &mut self.nodes[to].map;
            map.merge(entry);
            let seq = map.seq();
            if !map.merge(entry).is_empty() || map.seq() != seq {
                return Err(format!("merging {} again changed n{to}", line(entry)));
            }
        }
        let cursor = self.nodes[to].cursors.entry(from).or_default();
        if cursor.replica != message.replica {
            if message.since == 0 {
                *cursor = Cursor {
                    replica: message.replica,
                    seq: message.upto,
                };
            }
        } else if message.since <= cursor.seq {
            cursor.seq = cursor.seq.max(message.upto);
        }
        Ok(())
    }

    /// `Task::save`.
    fn save(&mut self, n: usize) -> Result<(), String> {
        let node = &mut self.nodes[n];
        let map = rmp_serde::to_vec_named(&node.map).map_err(|e| e.to_string())?;
        node.unsaved_load = false;
        node.unsaved_partial_page = false;
        node.saved = Some(Saved {
            map,
            seen: node.seen.clone(),
            cursors: node.cursors.clone(),
        });
        Ok(())
    }

    fn network_fault(&mut self, n: usize) {
        if self.cfg.offline && self.rng.chance(50) {
            let until = self.now + 1 + self.rng.below(60) as u64;
            self.note(|| format!("n{n} offline until t{until}"));
            self.down_until[n] = until;
        } else if self.cfg.partitions {
            self.split_until = self.now + 1 + self.rng.below(60) as u64;
            for at in 0..self.cfg.nodes {
                self.group[at] = self.rng.below(3);
            }
            let (group, until) = (self.group.clone(), self.split_until);
            self.note(|| format!("network split into {group:?} until t{until}"));
        }
    }

    fn restart(&mut self, n: usize) -> Result<(), String> {
        let mut kinds = Vec::new();
        if self.cfg.restarts {
            kinds.push("restarts");
        }
        if self.cfg.crashes && !self.nodes[n].unsaved_load && !self.nodes[n].unsaved_partial_page {
            kinds.push("crashes");
        }
        if self.cfg.lost_state {
            kinds.push("loses its state");
        }
        if kinds.is_empty() {
            return Ok(());
        }
        let kind = kinds[self.rng.below(kinds.len())];
        self.note(|| format!("n{n} {kind}"));
        self.tally.restarts += 1;
        match kind {
            "restarts" => self.save(n)?,
            "crashes" => self.tally.crashes += 1,
            _ => self.nodes[n].saved = None,
        }
        // Its connections end. What it sent may still arrive, not what was
        // sent to it, which went to a process that is gone.
        self.streams
            .retain(|&(client, server), _| client != n && server != n);
        self.net.retain(|message| message.to != n);
        let before: Vec<Entry> = self.nodes[n].map.entries().cloned().collect();
        match self.nodes[n].saved.clone() {
            Some(saved) => {
                let map = rmp_serde::from_slice::<Map>(&saved.map)
                    .map_err(|e| e.to_string())?
                    .loaded();
                if kind == "restarts" && !map.entries().eq(before.iter()) {
                    return Err(format!("n{n} lost entries over a restart"));
                }
                let node = &mut self.nodes[n];
                node.unsaved_load = true;
                node.map = map;
                node.seen = saved.seen;
                node.cursors = saved.cursors;
            }
            None => {
                let replica = self.next_replica;
                self.next_replica += 1;
                let node = &mut self.nodes[n];
                node.map = Map::new(replica);
                node.seen.clear();
                node.cursors.clear();
            }
        }
        self.scan(n);
        Ok(())
    }

    /// Faults end, everything in flight arrives, and every node pulls from
    /// every other until nothing moves.
    fn settle(&mut self) -> Result<(), String> {
        self.note(|| "faults end".to_owned());
        self.split_until = 0;
        self.down_until.fill(0);
        while !self.net.is_empty() {
            self.now += 1;
            let at = if self.cfg.reorder {
                self.rng.below(self.net.len())
            } else {
                0
            };
            let message = self.net.remove(at);
            self.receive(&message)?;
        }
        self.streams.clear();
        self.settling = true;
        for _ in 0..SETTLE_ROUNDS {
            let before: Vec<u64> = self.nodes.iter().map(|node| node.map.seq()).collect();
            for client in 0..self.cfg.nodes {
                for server in 0..self.cfg.nodes {
                    if client != server {
                        self.drain(client, server)?;
                    }
                }
            }
            if self
                .nodes
                .iter()
                .zip(&before)
                .all(|(node, seq)| node.map.seq() == *seq)
            {
                for n in 0..self.cfg.nodes {
                    self.apply(n, true);
                }
                return Ok(());
            }
        }
        Err(format!("not settled after {SETTLE_ROUNDS} rounds"))
    }

    /// One whole pull. A client that got to the end of a server's changes has
    /// seen all that server holds, or `since` skipped something.
    fn drain(&mut self, client: usize, server: usize) -> Result<(), String> {
        self.streams.remove(&(client, server));
        while let Some(message) = self.page(client, server)? {
            self.receive(&message)?;
        }
        self.streams.remove(&(client, server));
        for entry in self.nodes[server].map.entries() {
            let held = self.nodes[client].map.get(&entry.path);
            if !held.is_some_and(|held| has_seen(&held.version, &entry.version)) {
                return Err(format!(
                    "n{client} pulled all of n{server} but is behind on {}: has {}",
                    line(entry),
                    held.map_or("nothing".to_owned(), line)
                ));
            }
        }
        Ok(())
    }

    fn check(&mut self) -> Result<(), String> {
        for n in 0..self.cfg.nodes {
            let list: Vec<String> = self.nodes[n].map.entries().map(line).collect();
            self.note(|| format!("n{n} ends with: {}", list.join(", ")));
        }
        self.check_convergence()?;
        self.check_paging()?;
        self.check_no_edit_lost()?;
        self.check_deletes()?;
        self.check_conflict_names()?;
        self.check_disks()?;
        self.check_any_order()?;
        for entry in self.nodes[0].map.entries() {
            self.tally.conflicts += u64::from(is_conflict(&entry.path) && entry.file.is_some());
            self.tally.tombstones += u64::from(entry.file.is_none());
        }
        Ok(())
    }

    fn check_convergence(&self) -> Result<(), String> {
        for (n, node) in self.nodes.iter().enumerate().skip(1) {
            if let Some(differ) = difference(&self.nodes[0].map, &node.map) {
                return Err(format!("n0 and n{n} differ: {differ}"));
            }
        }
        Ok(())
    }

    /// Paging through a list in small steps gives all of it, once each.
    fn check_paging(&self) -> Result<(), String> {
        for (n, node) in self.nodes.iter().enumerate() {
            let (mut paged, mut since) = (Vec::new(), 0);
            loop {
                let changes = node.map.since(since, self.cfg.page);
                if changes.entries.is_empty() {
                    break;
                }
                since = changes.upto;
                paged.extend(changes.entries);
            }
            if since > node.map.seq() {
                return Err(format!("n{n} paged to {since} of {}", node.map.seq()));
            }
            paged.sort_by(|a, b| a.path.cmp(&b.path));
            if !paged.iter().eq(node.map.entries()) {
                return Err(format!("n{n} paging gave {} entries", paged.len()));
            }
        }
        Ok(())
    }

    /// Every content written is at its path or a copy of it, unless a node
    /// with that content in front of it replaced it.
    fn check_no_edit_lost(&self) -> Result<(), String> {
        // Two losers of one path whose hashes start alike (one pair in 2^32)
        // share a copy's name, and a version that has seen one replaces it.
        if self.cfg.tag_collisions {
            return Ok(());
        }
        // A crash can undo a write of content the list held before under an
        // older version, or leave content moved to another path that wants it,
        // so there it is enough that the content was seen, or is anywhere.
        let rewound = self.cfg.crashes || self.cfg.lost_state;
        let map = &self.nodes[0].map;
        for write in &self.writes {
            // The rest of `.git` keeps no loser.
            let Some(copy) = conflict_path(&write.path, &write.hash) else {
                continue;
            };
            // A ref is no file to move aside: one replaced before its copy
            // is written stays only in the reflog if the list is then lost.
            if gitdir::in_git(&write.path) && rewound {
                continue;
            }
            // Not promised once someone edits a copy: its version counts edits
            // of the original too, so at the copy's path an edit and a later
            // copy of the same content can each pass for newer and replace
            // the other unseen.
            if is_conflict(&write.path) || self.overwrites.iter().any(|over| over.path == copy) {
                continue;
            }
            let family_of = family(&write.path);
            let replaced = self.overwrites.iter().any(|over| {
                over.saw == Some(write.hash)
                    && family(&over.path) == family_of
                    && (rewound || has_seen(&over.base, &write.version))
            });
            let kept = || {
                map.entries().any(|entry| {
                    entry.file.as_ref().is_some_and(|f| f.hash == write.hash)
                        && (rewound || family(&entry.path) == family_of)
                })
            };
            if !replaced && !kept() {
                return Err(format!(
                    "n{} wrote {} at {} {:?} and it is gone",
                    write.node,
                    tag(&write.hash),
                    write.path,
                    write.version
                ));
            }
        }
        Ok(())
    }

    /// A path ends deleted only if every content written there had been seen
    /// by a later edit or delete of it.
    fn check_deletes(&self) -> Result<(), String> {
        // Versions lost with a list say nothing of what a node had seen.
        if self.cfg.crashes || self.cfg.lost_state {
            return Ok(());
        }
        let map = &self.nodes[0].map;
        for write in &self.writes {
            let deleted = map
                .get(&write.path)
                .is_some_and(|entry| entry.file.is_none());
            let seen = self
                .overwrites
                .iter()
                .any(|over| over.path == write.path && has_seen(&over.base, &write.version));
            if deleted && !seen {
                return Err(format!(
                    "{} ended deleted without any node seeing n{}'s {} {:?}",
                    write.path,
                    write.node,
                    tag(&write.hash),
                    write.version
                ));
            }
        }
        Ok(())
    }

    /// A copy's name follows from its original and content alone, and names
    /// stack up only when people edit copies or tags collide.
    fn check_conflict_names(&self) -> Result<(), String> {
        let map = &self.nodes[0].map;
        let limit = if self.cfg.edit_copies || self.cfg.tag_collisions {
            1 + self.authored.len()
        } else {
            1
        };
        let copies = map.entries().filter(|e| is_conflict(&e.path)).count();
        if copies > self.authored.len() {
            return Err(format!(
                "{copies} copies from {} writes",
                self.authored.len()
            ));
        }
        for entry in map.entries().filter(|e| is_conflict(&e.path)) {
            if depth(&entry.path) > limit {
                return Err(format!("{} is nested too deep", entry.path));
            }
            if let (Some(file), false) = (&entry.file, self.cfg.edit_copies) {
                let original = original_of(&entry.path);
                let named = original.and_then(|original| conflict_path(&original, &file.hash));
                if named.as_deref() != Some(&entry.path) {
                    return Err(format!("{} holds {}", entry.path, tag(&file.hash)));
                }
            }
        }
        Ok(())
    }

    /// The folders end up as the list says.
    fn check_disks(&self) -> Result<(), String> {
        for (n, node) in self.nodes.iter().enumerate() {
            for entry in node.map.entries() {
                let hash = node.disk.get(&entry.path).map(|file| file.hash);
                let kept = gitdir::is_kept_from_deletes(&entry.path);
                if hash != entry.file.as_ref().map(|file| file.hash)
                    && !(kept && entry.file.is_none())
                {
                    let held = hash.map_or("nothing".to_owned(), |hash| tag(&hash));
                    return Err(format!("n{n} has {held} for {}", line(entry)));
                }
            }
        }
        Ok(())
    }

    /// The entries `edit` made, shuffled into fresh lists, settle on one list
    /// once those lists swap what they derived, and any entry that arrives
    /// late changes nothing. A crash can lose an entry on every node, making
    /// it new again, so the nodes' list is only asked without crashes.
    fn check_any_order(&mut self) -> Result<(), String> {
        let mut maps: Vec<Map> = (1..=3).map(Map::new).collect();
        for map in &mut maps {
            let mut entries: Vec<&Entry> = self.authored.iter().collect();
            self.rng.shuffle(&mut entries);
            for entry in entries {
                map.merge(entry);
            }
        }
        let mut settled = false;
        for _ in 0..SETTLE_ROUNDS {
            let before: Vec<u64> = maps.iter().map(Map::seq).collect();
            for from in 0..maps.len() {
                let entries = maps[from].since(0, usize::MAX).entries;
                for (to, map) in maps.iter_mut().enumerate() {
                    if to != from {
                        for entry in &entries {
                            map.merge(entry);
                        }
                    }
                }
            }
            if maps.iter().map(Map::seq).eq(before.iter().copied()) {
                settled = true;
                break;
            }
        }
        if !settled {
            return Err("shuffled lists did not settle".to_owned());
        }
        for other in &maps[1..] {
            if let Some(differ) = difference(&maps[0], other) {
                return Err(format!("shuffled lists differ: {differ}"));
            }
        }
        let rewound = self.cfg.crashes || self.cfg.lost_state;
        for entry in &self.authored {
            if !maps[0].merge(entry).is_empty() {
                return Err(format!("late {} changed a settled list", line(entry)));
            }
            if !rewound && !self.nodes[0].map.merge(entry).is_empty() {
                return Err(format!("late {} changed the nodes' list", line(entry)));
            }
        }
        Ok(())
    }
}

struct Outcome {
    config: String,
    trace: Vec<String>,
    result: Result<(), String>,
    tally: Tally,
}

fn run(seed: u64, trace: bool) -> Outcome {
    let mut sim = Sim::new(seed, trace);
    let config = format!("{:?}", sim.cfg);
    let result = catch_unwind(AssertUnwindSafe(|| sim.run()))
        .unwrap_or_else(|_| Err("the simulator panicked".to_owned()));
    Outcome {
        config,
        trace: sim.trace.take().unwrap_or_default(),
        result,
        tally: sim.tally,
    }
}

fn replay_command(seed: u64) -> String {
    format!("BEEP_SIM_SEED={seed} cargo test -p beep-node --lib map_sim -- --nocapture")
}

/// All a person needs about a failed seed.
fn report(seed: u64, outcome: &Outcome) -> String {
    let failure = match &outcome.result {
        Ok(()) => "passed",
        Err(e) => e,
    };
    format!(
        "seed {seed}: {failure}\n{}\ntrace:\n  {}\nseed {seed}: {failure}\nreplay: {}",
        outcome.config,
        outcome.trace.join("\n  "),
        replay_command(seed)
    )
}

/// Runs `seeds` on all cores, giving the first that fails.
fn run_many(seeds: impl Iterator<Item = u64> + Send) -> Result<Tally, u64> {
    let seeds = Mutex::new(seeds);
    let failed = AtomicU64::new(0);
    let stop = AtomicBool::new(false);
    let total = Mutex::new(Tally::default());
    let cores = thread::available_parallelism().map_or(1, |n| n.get());
    thread::scope(|scope| {
        for _ in 0..cores {
            scope.spawn(|| {
                let mut mine = Tally::default();
                while !stop.load(Ordering::Relaxed) {
                    let Some(seed) = seeds.lock().expect("no worker panics").next() else {
                        break;
                    };
                    let outcome = run(seed, false);
                    mine.add(outcome.tally);
                    if outcome.result.is_err() {
                        failed.store(seed, Ordering::Relaxed);
                        stop.store(true, Ordering::Relaxed);
                    }
                }
                total.lock().expect("no worker panics").add(mine);
            });
        }
    });
    if stop.load(Ordering::Relaxed) {
        return Err(failed.load(Ordering::Relaxed));
    }
    Ok(total.into_inner().expect("no worker panics"))
}

fn env_u64(name: &str) -> Option<u64> {
    let value = env::var(name).ok()?;
    Some(
        value
            .parse()
            .unwrap_or_else(|_| panic!("{name} must be a number")),
    )
}

#[test]
fn map_sim_seeds_hold() {
    if let Some(seed) = env_u64("BEEP_SIM_SEED") {
        let outcome = run(seed, true);
        println!("{}", report(seed, &outcome));
        assert!(outcome.result.is_ok(), "seed {seed} failed");
        return;
    }
    let count = env_u64("BEEP_SIM_SEEDS").unwrap_or(SEEDS);
    let started = Instant::now();
    let seeds = FAILED_SEEDS.iter().copied().chain(0..count);
    match run_many(seeds) {
        Ok(tally) => {
            println!("{count} seeds in {:?}: {tally:?}", started.elapsed());
            // The seeds must keep reaching what the checks are about.
            assert!(tally.conflicts > 0 && tally.tombstones > 0 && tally.dropped > 0);
            assert!(tally.restarts > 0 && tally.crashes > 0);
        }
        Err(seed) => panic!("{}", report(seed, &run(seed, true))),
    }
}

#[test]
fn map_sim_replays_exactly() {
    for seed in 0..50 {
        let (first, second) = (run(seed, true), run(seed, true));
        assert_eq!(first.trace, second.trace, "seed {seed}");
        assert_eq!(first.result, second.result, "seed {seed}");
    }
}
