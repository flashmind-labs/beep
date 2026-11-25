//! Claude Code sessions started outside Beep, in a terminal or an IDE
//! (`External`): finding them, reading their transcripts and following them.
//! Taking one over runs it through the ACP adapter.

use crate::{
    agents::found::{Seen as Sighting, folder_place::of_folder},
    machine::wsl::{self, DistroName},
};
use anyhow::{Context, Result, bail};
use beep_proto::SessionId;
use beep_proto::node::{
    LinkRx, LinkTx,
    agent::{ExternalSession, LiveStatus, Presence, Steer, WorkPlace},
};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, LazyLock, Mutex, OnceLock},
    time::{Duration, Instant},
};
use tokio::task::spawn_blocking;
use crate::agents::window::Excerpt;



mod discover;
pub mod hooks;
mod live;
mod procs;
mod subagents;
mod transcript;
mod watch;

/// The most sessions one listing reads.
const MAX_LISTED: usize = 50;
/// How many a listing reads when the server names no number.
const DEFAULT_LISTED: usize = 20;

/// Claude Code's sessions on this machine, and the hook events about them.
pub struct External {
    /// Claude Code's folder, where the store is once its `projects` exists.
    root: Option<PathBuf>,
    store: OnceLock<discover::Store>,
    hub: watch::Hub,
    live: live::Live,
    /// The transcripts read last, parsed.
    transcripts: Arc<transcript::Transcripts>,
}

/// How many sessions' parsed transcripts are kept at once: enough for every
/// session a desktop follows off screen and the ones it shows.
const TRANSCRIPTS_KEPT: usize = 16;

impl Default for External {
    fn default() -> Self {
        Self {
            root: None,
            store: OnceLock::new(),
            hub: watch::Hub::default(),
            live: live::Live::default(),
            transcripts: Arc::new(transcript::Transcripts::new(TRANSCRIPTS_KEPT)),
        }
    }
}

impl External {
    pub(crate) async fn attach_children(
        &self,
        id: &SessionId,
        calls: Vec<(usize, String)>,
        mut entries: Vec<beep_proto::node::agent::Entry>,
    ) -> Result<Vec<beep_proto::node::agent::Entry>> {
        let Some(path) = self.file(id).await? else {
            return Ok(entries);
        };
        let kept = self.transcripts.clone();
        spawn_blocking(move || {
            subagents::attach_live(&path, &calls, &mut entries, &kept);
            entries
        })
        .await
        .context("reading native child conversations")
    }
    pub(crate) fn follow_paths(&self) -> Vec<PathBuf> {
        self.root
            .as_ref()
            .into_iter()
            .flat_map(|root| [root.join("projects"), root.join("sessions")])
            .collect()
    }

    pub(crate) fn presence_path(&self) -> Option<PathBuf> {
        self.root.as_ref().map(|root| root.join("sessions"))
    }

    /// Claude Code's stores: this machine's, then each WSL distribution's,
    /// with the distribution it is in.
    fn stores(&self) -> Vec<(Option<DistroName>, discover::Store)> {
        let mut stores: Vec<_> = self
            .store()
            .map(|store| (None, store))
            .into_iter()
            .collect();
        stores.extend(
            wsl_stores()
                .into_iter()
                .map(|(distro, store)| (Some(distro), store)),
        );
        stores
    }

    /// Looks for Claude Code's folder; without one there is nothing to list.
    pub fn locate() -> Self {
        Self {
            root: crate::agents::claude_dir::config_dir(),
            ..Self::default()
        }
    }

    /// The store, looked for again while Claude Code has not made its
    /// `projects` folder yet (one stat per call).
    fn store(&self) -> Option<discover::Store> {
        if let Some(store) = self.store.get() {
            return Some(store.clone());
        }

        let found = discover::Store::in_root(self.root.as_deref()?)?;
        Some(self.store.get_or_init(|| found).clone())
    }

    /// The sessions in the Claude Code folder `root`, which may not have its
    /// `projects` folder yet.
    #[cfg(test)]
    pub(crate) fn locate_in(root: &Path) -> Self {
        Self {
            root: Some(root.to_owned()),
            ..Self::default()
        }
    }

    /// The sessions in the Claude Code folder `root`.
    #[cfg(test)]
    pub(crate) fn at(root: &Path) -> Self {
        Self {
            store: OnceLock::from(discover::Store::at(root)),
            ..Self::default()
        }
    }

    /// The newest `limit` sessions, or only `only`. `owned` maps the
    /// sessions this node runs, by Claude Code's id, to the node's id.
    pub(crate) async fn list(
        &self,
        limit: u32,
        only: Option<SessionId>,
        owned: HashMap<SessionId, SessionId>,
    ) -> Result<Vec<ExternalSession>> {
        let limit = crate::agents::window::clamp_limit(limit, DEFAULT_LISTED, MAX_LISTED);
        let mut sessions = self
            .read_stores(owned, move |store, snapshot| {
                discover::list(store, snapshot, limit, only.as_ref())
            })
            .await?;
        sessions.sort_by_key(|session| std::cmp::Reverse(session.last_activity));
        sessions.truncate(limit);
        Ok(sessions)
    }

    /// Every session with the folder it ran in, for `crate::agents::found`.
    pub(crate) async fn sightings(&self) -> Result<Vec<Sighting>> {
        let stores = self.stores();
        spawn_blocking(move || {
            let mut seen = Vec::new();

            for (distro, store) in &stores {
                let mut found = discover::sightings(store)?;
                for one in &mut found {
                    one.cwd = wsl_folder(distro.as_ref(), std::mem::take(&mut one.cwd));
                }
                seen.extend(found);
            }

            Ok::<_, std::io::Error>(seen)
        })
        .await?
        .context("reading Claude Code's sessions")
    }

    /// The sessions named in `ids`, newest first, each with its title, else
    /// its last prompt. `owned` is as for `list`.
    pub(crate) async fn named(
        &self,
        ids: HashSet<SessionId>,
        owned: HashMap<SessionId, SessionId>,
    ) -> Result<Vec<ExternalSession>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut sessions = self
            .read_stores(owned, move |store, snapshot| {
                discover::list_ids(store, snapshot, &ids)
            })
            .await?;

        for session in &mut sessions {
            if session.title.is_none() && !session.last_prompt.is_empty() {
                session.title = crate::agents::tags::title(&session.last_prompt);
            }
        }

        Ok(sessions)
    }

    async fn read_stores(
        &self,
        owned: HashMap<SessionId, SessionId>,
        pick: impl Fn(&discover::Store, &discover::Snapshot) -> std::io::Result<Vec<ExternalSession>>
        + Send
        + 'static,
    ) -> Result<Vec<ExternalSession>> {
        let stores = self.stores();

        if stores.is_empty() {
            return Ok(Vec::new());
        }

        let processes = procs::claude_processes().await;
        spawn_blocking(move || {
            let mut sessions = Vec::new();

            for (distro, store) in &stores {
                let snapshot = discover::Snapshot::of(store, processes.clone(), owned.clone());
                let mut listed = pick(store, &snapshot)?;
                listed
                    .iter_mut()
                    .for_each(|session| in_windows(distro.as_ref(), session));
                sessions.extend(listed);
            }

            Ok::<_, std::io::Error>(sessions)
        })
        .await?
        .context("reading Claude Code's sessions")
    }

    /// Native checkout folders for sessions the node already owns, without
    /// discovering processes or scanning unrelated session files.
    pub(crate) async fn places(
        &self,
        ids: HashSet<SessionId>,
    ) -> Result<HashMap<SessionId, WorkPlace>> {
        let stores = self.stores();

        spawn_blocking(move || {
            let mut places = HashMap::new();

            for (distro, store) in stores {
                for (id, folder) in discover::folders(&store, &ids) {
                    let folder = wsl_folder(distro.as_ref(), folder);

                    if let Some(place) = of_folder(&folder) {
                        places.insert(id, place);
                    }
                }
            }

            places
        })
        .await
        .context("reading native session folders")
    }

    /// The last `last` entries of session `id`'s transcript, how many came
    /// before them and the pictures they show; `None` when there is no
    /// such session.
    pub(crate) async fn transcript(
        &self,
        id: &SessionId,
        last: usize,
        before: Option<u64>,
    ) -> Result<Option<Excerpt>> {
        let Some(path) = self.file(id).await? else {
            return Ok(None);
        };
        Ok(Some(self.page(path, last, before).await?))
    }

    /// Number of native entries before the first retained replay entry.
    pub(super) async fn prefix_len(
        &self,
        path: PathBuf,
        replay: Vec<beep_proto::node::agent::Entry>,
    ) -> Result<u64> {
        let kept = self.transcripts.clone();
        Ok(spawn_blocking(move || {
            kept.with(&path, |digest| {
                transcript::prefix_len(&digest.entries, &replay) as u64
            })
        })
        .await??)
    }

    pub(super) async fn page(
        &self,
        path: PathBuf,
        last: usize,
        before: Option<u64>,
    ) -> Result<Excerpt> {
        let kept = self.transcripts.clone();
        Ok(spawn_blocking(move || transcript::entries(&path, last, before, &kept)).await??)
    }

    /// Session `id`'s transcript file, when there is one.
    pub(crate) async fn file(&self, id: &SessionId) -> Result<Option<PathBuf>> {
        let stores = self.stores();
        let started = std::time::Instant::now();
        let id = id.to_owned();
        let found =
            spawn_blocking(move || stores.iter().find_map(|(_, store)| store.find(&id))).await?;
        crate::agents::transcript_stage("find", started);
        Ok(found)
    }

    /// Starts following session `id` by its file, found once. `None` when
    /// it has none of its own here, such as a subagent's conversation.
    pub(crate) async fn follow(&self, id: &SessionId) -> Result<Option<Followed>> {
        if !discover::is_session_id(id.as_ref()) {
            return Ok(None);
        }

        Ok(self.file(id).await?.map(|path| Followed {
            id: id.clone(),
            path,
            machine: None,
        }))
    }

    /// The session `followed` reads, as `list` describes it. The processes
    /// and pid files are read again only when `stale` says they may have
    /// changed, or once they are `MACHINE_KEPT` old: a session a new process
    /// resumes may write no pid file.
    pub(crate) async fn followed(
        &self,
        followed: &mut Followed,
        stale: bool,
        owned: HashMap<SessionId, SessionId>,
    ) -> Result<ExternalSession> {
        let Some(store) = self.store() else {
            bail!("session is no longer on the machine");
        };

        let machine = match followed.machine.take() {
            Some((machine, read)) if !stale && read.elapsed() < MACHINE_KEPT => (machine, read),
            _ => {
                let processes = procs::claude_processes().await;
                let s = store.clone();
                let machine =
                    spawn_blocking(move || discover::Snapshot::of(&s, processes, HashMap::new()))
                        .await?;
                (machine, Instant::now())
            }
        };
        let snapshot = Arc::new(discover::Snapshot {
            owned,
            ..machine.0.clone()
        });
        followed.machine = Some(machine);
        let describe = |path: PathBuf| {
            let (store, snapshot) = (store.clone(), snapshot.clone());
            spawn_blocking(move || discover::describe_file(&store, &path, &snapshot))
        };
        let mut found = describe(followed.path.clone())
            .await?
            .context("reading Claude Code's session")?;

        // Its file moved, as when its folder was renamed.
        if found.is_none()
            && let Some(path) = self.file(&followed.id).await?
            && path != followed.path
        {
            followed.path = path;
            found = describe(followed.path.clone())
                .await?
                .context("reading Claude Code's session")?;
        }

        found.context("session is no longer on the machine")
    }

    /// The last `last` entries of the transcript `followed` reads, none
    /// before entry `from` when it is given and the transcript reaches it.
    pub(crate) async fn followed_transcript(
        &self,
        followed: &Followed,
        last: usize,
        from: Option<u64>,
    ) -> Result<Excerpt> {
        let path = followed.path.clone();
        let kept = self.transcripts.clone();
        Ok(spawn_blocking(move || match from {
            Some(from) => transcript::newest_from(&path, last, from, &kept),
            None => transcript::entries(&path, last, None, &kept),
        })
        .await??)
    }

    /// Deletes session `id`'s transcript and its side folder; `false` when
    /// there is no such transcript. Both are found through the store and
    /// must lie inside its projects folder.
    pub(crate) async fn delete(&self, id: &SessionId) -> Result<bool> {
        let Some(store) = self.store() else {
            return Ok(false);
        };
        let id = id.to_owned();
        spawn_blocking(move || {
            let Some(file) = store.find(&id) else {
                return Ok(false);
            };
            let projects = store.projects();
            crate::agents::found::remove_file_in(&projects, &file)?;

            if let Some(side) = file.parent().map(|folder| folder.join(id.as_ref())) {
                crate::agents::found::remove_dir_in(&projects, &side)?;
            }

            Ok(true)
        })
        .await?
    }

    /// Takes hook events on the socket in `dir` until the node stops.
    pub async fn listen_hooks(&self, dir: &Path) -> Result<()> {
        watch::listen(&hooks::socket_path(dir), &self.hub, &self.live).await
    }

    /// How session `id` does now as its hooks report it, after queuing
    /// `steer` for its next hook when given. `owned` is as for `list`.
    pub async fn live(
        &self,
        id: &SessionId,
        steer: Option<Steer>,
        owned: HashMap<SessionId, SessionId>,
    ) -> Result<LiveStatus> {
        let found = match self.list(1, Some(id.clone()), owned).await?.pop() {
            Some(found) => found,
            None if self.live.knows(id) => ExternalSession {
                id: id.clone(),
                agent: "claude".to_owned(),
                ..ExternalSession::default()
            },
            None => bail!("there is no Claude Code session {id} here"),
        };

        if let Some(taken) = &found.taken_over {
            bail!("session {id} runs here as session {taken}, so steer it as that session");
        }

        if let Some(steer) = steer {
            self.live.steer(id, steer).map_err(anyhow::Error::msg)?;
        }

        Ok(self.live.status(found))
    }

    /// Serves one `Watch` stream.
    pub(crate) async fn watch(
        &self,
        tx: LinkTx,
        rx: LinkRx,
        sessions: Vec<SessionId>,
        rings: tokio::sync::broadcast::Receiver<crate::events::Change>,
    ) -> Result<()> {
        watch::serve(tx, rx, self.store(), &self.hub, sessions, rings).await
    }
}

/// How long a follow reuses the processes and pid files it read, when
/// nothing says they changed.
const MACHINE_KEPT: Duration = Duration::from_secs(10);

/// A Claude Code session a reader follows: its file, and the processes and
/// pid files last read, with when.
pub(crate) struct Followed {
    id: SessionId,
    path: PathBuf,
    machine: Option<(discover::Snapshot, Instant)>,
}

impl Followed {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

/// Why `found` cannot be taken over, when it cannot.
pub(crate) fn refuse_takeover(found: &ExternalSession) -> Option<String> {
    if found.presence.is_live() {
        let process = found
            .pid
            .map(|pid| format!(" (process {pid})"))
            .unwrap_or_default();
        let how = match found.presence {
            Presence::Recent => "was active a moment ago".to_owned(),
            presence => format!("is {presence}"),
        };
        return Some(format!(
            "session {} {how} in a terminal or IDE{process}, so Beep can only follow it. \
             Once the user exits it there, it can be taken over.",
            found.id
        ));
    }

    if found.cwd.is_empty() {
        return Some(format!(
            "session {} does not record the folder it ran in",
            found.id
        ));
    }

    None
}

/// Claude Code's stores in the WSL distributions, each read once while it
/// has its `projects` folder.
fn wsl_stores() -> Vec<(DistroName, discover::Store)> {
    static KEPT: LazyLock<Mutex<HashMap<PathBuf, discover::Store>>> = LazyLock::new(Mutex::default);

    crate::agents::found::in_wsl_homes(&KEPT, |home| {
        discover::Store::in_root(&home.join(".claude"))
    })
}

fn wsl_folder(distro: Option<&DistroName>, folder: PathBuf) -> PathBuf {
    match distro {
        Some(d) => wsl::windows_folder(d, &folder.to_string_lossy()),
        None => folder,
    }
}

/// `session`, read from the store in `distro`, with its folder as Windows
/// names it.
fn in_windows(distro: Option<&DistroName>, session: &mut ExternalSession) {
    if distro.is_some() {
        let folder = wsl_folder(distro, PathBuf::from(&session.cwd));
        session.place = of_folder(&folder);
        session.cwd = folder.to_string_lossy().into_owned();
    }
}

#[cfg(test)]
#[path = "../../tests/agents_claude.rs"]
mod tests;
