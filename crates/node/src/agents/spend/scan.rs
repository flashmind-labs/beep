//! One pass over the logs: lists them, reads what each JSONL log gained
//! since the last pass, and reads a database again when it changed. Every
//! pass is bounded in files and bytes; what is left waits for the next.

use super::{
    Line, agy,
    billing::{AccountFiles, Accounts},
    claude, codex, databases,
    ledger::{Database, Ledger, LogFile, Part, Stamp},
    pi,
};
use crate::agents::found::{self, Agent, LogRoots, is_jsonl, is_rollout, wal_path, walk_files};
use beep_proto::SessionId;
use std::{
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet, HashSet},
    fs::{self, File},
    io::{self, BufRead, BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

/// The most log files listed.
const MAX_LISTED: usize = 20_000;
/// The most log files read in one pass.
const MAX_FILES: usize = 200;
/// The most bytes of logs read in one pass.
const MAX_BYTES: u64 = 256 * 1024 * 1024;
/// The longest line parsed; a longer one, a tool's output, is skipped.
const MAX_LINE: u64 = 16 * 1024 * 1024;
/// The most flsh databases looked at.
const MAX_DATABASES: usize = 300;

/// Where each agent's logs are.
#[derive(Debug, Clone, Default)]
pub(crate) struct Roots {
    /// Claude Code's `projects` folder.
    pub(crate) claude: Option<PathBuf>,
    /// Codex's `sessions` folder.
    pub(crate) codex: Option<PathBuf>,
    /// pi's sessions folder.
    pub(crate) pi: Option<PathBuf>,
    /// flsh's sessions folder, a database in each subfolder.
    pub(crate) flsh: Option<PathBuf>,
    /// OpenCode's database, or its data folder of them.
    pub(crate) opencode: Option<PathBuf>,
    /// Antigravity's conversations folder, a database each.
    pub(crate) agy: Option<PathBuf>,
    /// The files that tell how each agent signs in.
    pub(crate) accounts: AccountFiles,
}

impl Roots {
    pub(crate) fn locate() -> Self {
        let logs = LogRoots::locate();
        let accounts = AccountFiles {
            claude: crate::agents::claude_dir::config_dir(),
            codex: logs.codex.as_ref().map(|home| home.join("auth.json")),
            pi: logs.pi_agent.as_ref().map(|dir| dir.join("auth.json")),
            opencode: logs.opencode.as_deref().map(|root| {
                let data = if root.is_file() {
                    root.parent().unwrap_or(root)
                } else {
                    root
                };
                data.join("auth.json")
            }),
        };
        Self {
            claude: crate::agents::claude_dir::projects_dir(),
            codex: logs.codex.map(|home| home.join("sessions")),
            pi: logs.pi,
            flsh: logs.flsh,
            opencode: logs.opencode,
            agy: found::agy::conversations_dir(),
            accounts,
        }
    }
}

/// A log file found, and how it stands.
struct Listed {
    agent: Agent,
    path: PathBuf,
    stamp: Stamp,
}

fn retain_paths<V>(map: &mut BTreeMap<PathBuf, V>, keep: &HashSet<&Path>, changed: &mut bool) {
    let before = map.len();
    map.retain(|path, _| keep.contains(path.as_path()));
    *changed |= map.len() != before;
}

/// Reads what changed, logs written since `since` (Unix seconds) only.
pub(super) fn pass(ledger: &mut Ledger, roots: &Roots, since: u64) {
    let accounts = &mut Accounts::read(&roots.accounts);
    let mut listed = list_logs(roots, since);
    let paths: HashSet<&Path> = listed.iter().map(|log| log.path.as_path()).collect();
    retain_paths(&mut ledger.logs, &paths, &mut ledger.changed);
    listed.retain(|log| ledger.logs.get(&log.path).map(|read| read.stamp) != Some(log.stamp));
    listed.sort_by_key(|log| Reverse(log.stamp.modified_ns));
    let mut budget = MAX_BYTES;

    for log in listed.into_iter().take(MAX_FILES) {
        if budget == 0 {
            break;
        }

        let entry = ledger
            .logs
            .entry(log.path.clone())
            .or_insert_with(|| LogFile::new(log.agent));
        match read_log(entry, &log, &mut budget, accounts) {
            Ok(touched) => {
                ledger.changed = true;

                for session in touched {
                    ledger.touch(log.agent, session);
                }
            }
            Err(e) => log::debug!("agent log not read; path={} error={e}", log.path.display()),
        }
    }

    read_databases(ledger, roots, since, accounts);
}

/// Every log of every agent written since `since`.
fn list_logs(roots: &Roots, since: u64) -> Vec<Listed> {
    let mut out = Vec::new();

    if let Some(root) = &roots.claude {
        // `<project>/<session>.jsonl`, and a subagent's in
        // `<project>/<session>/subagents/`.
        for project in subfolders(root) {
            collect(Agent::Claude, &project, 0, &is_jsonl, since, &mut out);

            for session in subfolders(&project) {
                collect(
                    Agent::Claude,
                    &session.join("subagents"),
                    0,
                    &is_jsonl,
                    since,
                    &mut out,
                );
            }
        }
    }

    if let Some(root) = &roots.codex {
        collect(Agent::Codex, root, 3, &is_rollout, since, &mut out);
    }

    if let Some(root) = &roots.pi {
        collect(Agent::Pi, root, 1, &is_jsonl, since, &mut out);
    }

    out
}

fn subfolders(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path())
        .collect()
}

/// The files under `dir`, `depth` folders down, that `wanted` takes and
/// that were written since `since`.
fn collect(
    agent: Agent,
    dir: &Path,
    depth: usize,
    wanted: &dyn Fn(&Path) -> bool,
    since: u64,
    out: &mut Vec<Listed>,
) {
    walk_files(dir, depth, wanted, &mut |_, path| {
        if out.len() >= MAX_LISTED {
            return false;
        }
        if let Ok(stamp) = Stamp::of(&path)
            && stamp.modified_ns / 1_000_000_000 >= u128::from(since)
        {
            out.push(Listed { agent, path, stamp });
        }
        out.len() < MAX_LISTED
    });
}

/// Reads what `log` gained since `entry` last read it, at most `budget`
/// bytes, which it takes down. The sessions it added to.
fn read_log(
    entry: &mut LogFile,
    log: &Listed,
    budget: &mut u64,
    accounts: &mut Accounts,
) -> io::Result<BTreeSet<SessionId>> {
    let mut touched = BTreeSet::new();

    if log.stamp.len < entry.read {
        // Rewritten shorter: read it again from the start.
        touched.extend(entry.parts.keys().cloned());
        *entry = LogFile::new(log.agent);
    }

    let mut file = File::open(&log.path)?;
    file.seek(SeekFrom::Start(entry.read))?;
    let mut reader = BufReader::new(file);
    let mut line = Vec::new();

    loop {
        if *budget == 0 {
            // The rest waits; the stamp stays, so the next pass goes on.
            return Ok(touched);
        }

        line.clear();
        let got = (&mut reader).take(MAX_LINE).read_until(b'\n', &mut line)?;

        if got == 0 {
            break;
        }

        if !line.ends_with(b"\n") {
            if (got as u64) < MAX_LINE {
                // A line still being written.
                break;
            }

            let rest = skip_line(&mut reader)?;

            match rest {
                Some(skipped) => {
                    entry.read += got as u64 + skipped;
                    *budget = budget.saturating_sub(got as u64 + skipped);
                    continue;
                }
                None => break,
            }
        }

        entry.read += got as u64;
        *budget = budget.saturating_sub(got as u64);
        let Ok(text) = str::from_utf8(&line) else {
            continue;
        };

        if let Some(parsed) = parse(log.agent, text)
            && let Some(session) = entry.take(parsed, accounts)
        {
            touched.insert(session);
        }
    }

    entry.stamp = log.stamp;
    Ok(touched)
}

/// What the log at `path` says of its session, with none of what it spent:
/// the header's folder and provider, the model and the running totals a
/// request that follows needs.
pub(super) fn header_of(agent: Agent, path: &Path, accounts: &AccountFiles) -> io::Result<LogFile> {
    let mut entry = LogFile::new(agent);
    let log = Listed {
        agent,
        path: path.to_owned(),
        stamp: Stamp::of(path)?,
    };
    let mut budget = MAX_BYTES;
    read_log(&mut entry, &log, &mut budget, &mut Accounts::read(accounts))?;
    // What it spent was counted where it ran.
    entry.parts.clear();
    Ok(entry)
}

/// Skips the rest of an overlong line: the bytes skipped, newline
/// included, or `None` when the line is not finished yet.
fn skip_line(reader: &mut impl BufRead) -> io::Result<Option<u64>> {
    let mut skipped = 0u64;

    loop {
        let buffer = reader.fill_buf()?;

        if buffer.is_empty() {
            return Ok(None);
        }

        match buffer.iter().position(|&b| b == b'\n') {
            Some(at) => {
                reader.consume(at + 1);
                return Ok(Some(skipped + at as u64 + 1));
            }
            None => {
                let len = buffer.len();
                reader.consume(len);
                skipped += len as u64;
            }
        }
    }
}

fn parse(agent: Agent, text: &str) -> Option<Line> {
    match agent {
        Agent::Claude if claude::wanted(text) => claude::line(text),
        Agent::Codex if codex::wanted(text) => codex::line(text),
        Agent::Pi if pi::wanted(text) => pi::line(text),
        _ => None,
    }
}

/// The databases of OpenCode and flsh, and flsh's session files and
/// Antigravity's conversations written since `since`, each read whole like
/// a database. A database flsh has imported into session files is left
/// out, so nothing counts twice.
fn list_databases(roots: &Roots, since: u64) -> Vec<(Agent, PathBuf)> {
    let mut out: Vec<(Agent, PathBuf)> = Vec::new();

    if let Some(root) = &roots.opencode {
        out.extend(
            found::opencode_databases(root)
                .into_iter()
                .map(|path| (Agent::OpenCode, path)),
        );
    }

    if let Some(root) = &roots.flsh {
        out.extend(
            found::flsh_databases(root)
                .take(MAX_DATABASES)
                .map(|path| (Agent::Flsh, path)),
        );
        let mut files = Vec::new();
        collect(Agent::Flsh, root, 1, &is_jsonl, since, &mut files);
        out.extend(files.into_iter().map(|file| (Agent::Flsh, file.path)));
    }

    if let Some(root) = &roots.agy {
        let mut files = Vec::new();
        collect(Agent::Agy, root, 0, &found::agy::is_conversation, since, &mut files);
        out.extend(files.into_iter().map(|file| (Agent::Agy, file.path)));
    }

    out
}

/// A database's stamp, its write-ahead log counted: SQLite writes there
/// first.
fn database_stamp(path: &Path) -> io::Result<Stamp> {
    let main = Stamp::of(path)?;
    Ok(match Stamp::of(&wal_path(path)) {
        Ok(wal) => Stamp {
            len: main.len.saturating_add(wal.len),
            modified_ns: main.modified_ns.max(wal.modified_ns),
        },
        Err(_) => main,
    })
}

/// Reads each database that changed again, whole from `since`.
fn read_databases(ledger: &mut Ledger, roots: &Roots, since: u64, accounts: &mut Accounts) {
    let listed = list_databases(roots, since);
    let paths: HashSet<&Path> = listed.iter().map(|(_, path)| path.as_path()).collect();
    retain_paths(&mut ledger.databases, &paths, &mut ledger.changed);
    for (agent, path) in &listed {
        let Ok(stamp) = database_stamp(path) else {
            continue;
        };

        let flsh_file = *agent == Agent::Flsh && is_jsonl(path);
        let billed = path.with_extension("openrouter.json");
        let billed_stamp = if flsh_file {
            match Stamp::of(&billed) {
                Ok(stamp) => Some(stamp),
                Err(e) if e.kind() == io::ErrorKind::NotFound => None,
                Err(_) => continue,
            }
        } else {
            None
        };
        let previous = ledger.databases.get(path);
        if previous.is_some_and(|read| {
            read.stamp == stamp
                && read.billed_stamp == billed_stamp
                && (*agent != Agent::Flsh || read.separates_historical_usage)
        }) {
            continue;
        }

        let read = match agent {
            Agent::OpenCode => {
                databases::opencode(path, since, accounts).map_err(|e| e.to_string())
            }
            Agent::Agy => agy::conversation(path, since, accounts).map_err(|e| e.to_string()),
            _ if flsh_file => {
                let counted = previous.and_then(|entry| entry.counted.as_ref());
                databases::flsh_file(path, counted, accounts).map_err(|e| e.to_string())
            }
            _ => databases::flsh(path, since, accounts).map_err(|e| e.to_string()),
        };
        let entry = ledger.databases.entry(path.clone()).or_insert(Database {
            agent: Some(*agent),
            ..Database::default()
        });
        entry.stamp = stamp;
        entry.billed_stamp = billed_stamp;
        ledger.changed = true;

        match read {
            Ok(parts) => {
                entry.separates_historical_usage = *agent == Agent::Flsh;
                let changed = changed_sessions(&entry.parts, &parts);
                entry.parts = parts;

                for session in changed {
                    ledger.touch(*agent, session);
                }
            }
            Err(e) => log::debug!("agent database not read; path={} error={e}", path.display()),
        }
    }
}

/// The sessions whose parts differ between `before` and `after`.
fn changed_sessions(
    before: &BTreeMap<SessionId, Part>,
    after: &BTreeMap<SessionId, Part>,
) -> Vec<SessionId> {
    before
        .keys()
        .chain(after.keys())
        .filter(|session| before.get(*session) != after.get(*session))
        .cloned()
        .collect::<BTreeSet<SessionId>>()
        .into_iter()
        .collect()
}
