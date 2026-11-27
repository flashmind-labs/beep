//! What a project's task shares with the pulls and the node.

use super::{
    list::merge_peer,
    state::{Cursor, GitCopy, State, read_saved},
};
use crate::files::place::Folder;
use crate::files::sync::{
    disk::{self, Stat},
    gitdir::{self, BranchTip, Checkout, Oid, Repo, WriteGuard},
    ignored::IgnoredRoot,
    map::{Changes, Entry, Replica, is_conflict, original_of, valid_path},
    rules::Rules,
    scan::MAX_FILES,
    watch::Key,
};
use anyhow::{Context, Result, bail};
use beep_proto::node::sync::{Keep, ProjectStatus};
use beep_proto::watched;
use iroh::{EndpointAddr, EndpointId};
use iroh_blobs::Hash;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, RwLock},
    time::Duration,
};
use tokio::{
    sync::{Notify, watch},
    task::spawn_blocking,
    time::{Instant, timeout},
};
use tokio_util::sync::CancellationToken;

/// The longest a scan before matching a peer may take; the pull tries again.
const MATCH_SCAN: Duration = Duration::from_secs(30);

/// A project's request to run Git recovery.
#[derive(Default)]
pub(super) enum RecoveryTurn {
    /// Nothing asked, or the answer was taken.
    #[default]
    Idle,
    /// Published in the recoveries; no answer yet.
    Asked,
    Answered(Result<crate::files::holds::RepositoryHold>),
}

/// What a project's task shares with the pulls and the node.
pub(in crate::files::sync) struct Shared {
    pub(super) bootstrap_paths: RwLock<Option<HashSet<String>>>,
    pub(super) recovery: Arc<Mutex<Option<crate::files::holds::Recoveries>>>,
    /// Where this checkout is in asking for its recovery to be let run.
    pub(super) recovery_turn: Mutex<RecoveryTurn>,
    pub(super) name: String,
    pub(super) key: Key,
    pub(super) root: Arc<Folder>,
    pub(super) ignored: Option<IgnoredRoot>,
    pub(super) file: PathBuf,
    pub(super) replica: Replica,
    pub(super) state: Mutex<State>,
    pub(super) rules: Arc<RwLock<Rules>>,
    /// The project's sync rules from the mesh, read with its `.beepignore`.
    pub(in crate::files::sync) rules_text: String,
    /// The list's last change.
    pub(super) seq: watch::Sender<u64>,
    /// The latest list that can be sent without losing a peer's cursor on restart.
    pub(super) saved_seq: watch::Sender<u64>,
    pub(super) status: watch::Sender<ProjectStatus>,
    pub(super) peers: watch::Sender<Vec<EndpointAddr>>,
    /// Wakes the task when a pull merged something.
    pub(super) merged: Notify,
    /// Wakes the task to scan when this node changed the folder itself.
    pub(super) touched: Notify,
    /// Wakes the task to scan at once, for `rescan`.
    pub(super) forced: Notify,
    /// Wakes the task to bring the list in line before a peer is matched.
    pub(super) matching: Notify,
    /// Raised when a peer's advertised commits or branch tips change.
    pub(super) peer_commits: watch::Sender<u64>,
    /// How many scans `forced` asked for are done.
    pub(super) rescans: watch::Sender<u64>,
    /// Paused by the server: no scans, fetches or pulls.
    pub(super) paused: watch::Sender<bool>,
    pub(super) stop: CancellationToken,
    /// The peers yet to send what they had, or to prove out of reach. Until
    /// they have, this node cannot say it is in sync.
    unheard: Mutex<HashSet<EndpointId>>,
    /// When each peer last sent all it had, from a pull that went through.
    /// Unlike `unheard`, a pull that failed does not set it.
    caught: Mutex<HashMap<EndpointId, Instant>>,
    /// Commits peers advertised on their current pulls.
    pub(super) peer_heads: Mutex<HashMap<EndpointId, (Checkout, Option<String>)>>,
    pub(super) peer_branches: Mutex<HashMap<EndpointId, Vec<BranchTip>>>,
    /// When the last round that brought the folder in line with the list
    /// began. It saw every merge made before that.
    applied: Mutex<Instant>,
    /// Raised to end the pulls, so each starts again from the peer's list.
    pull_again: watch::Sender<u64>,
}

/// What is needed to start a project.
pub(in crate::files::sync) struct Start {
    pub(in crate::files::sync) name: String,
    pub(in crate::files::sync) key: Key,
    pub(in crate::files::sync) root: Arc<Folder>,
    pub(in crate::files::sync) peers: Vec<EndpointAddr>,
    /// Where its state is kept.
    pub(in crate::files::sync) file: PathBuf,
    pub(in crate::files::sync) paused: bool,
    /// The project's sync rules from the mesh.
    pub(in crate::files::sync) rules: String,
}

impl Shared {
    /// Loads the project's state; blocks.
    pub(in crate::files::sync) fn load(start: Start) -> Result<Arc<Self>> {
        let mut saved = read_saved(&start.file);
        let migrated = saved.git_copy.is_none();

        if migrated {
            saved.git_copy = Some(if start.root.path().join(gitdir::GIT).exists() {
                GitCopy::Finished
            } else {
                GitCopy::Pending { peer: None }
            });
            // Existing indexes, including staged work, stay owned by Git.
            saved.index_stale = false;
        }

        let ignored = IgnoredRoot::parse(&start.name, start.root.path())?;
        let git_only = ignored.is_none()
            && (saved.git_only
                || start.root.path().join(gitdir::GIT).exists()
                || saved.checkout.is_some_and(|checkout| checkout.present)
                || matches!(&saved.git_copy, Some(GitCopy::Finished)));
        let mode_changed = saved.git_only != git_only;
        saved.git_only = git_only;
        let mut state = State::new(saved);
        state.unsaved |= migrated || mode_changed;
        let rules = match &ignored {
            Some(scope) => scope.rules(start.root.path())?,
            None => Rules::load(start.root.path(), &start.rules)?,
        };
        let replica = state.saved.map.replica();
        let seq = state.saved.map.seq();
        let unheard = start.peers.iter().map(|addr| addr.id).collect();
        let status = ProjectStatus {
            name: start.name.clone(),
            folder: start.root.path().to_string_lossy().into_owned(),
            ..ProjectStatus::default()
        };
        let mut peer_branches = HashMap::new();

        if matches!(state.saved.git_copy, Some(GitCopy::Finished))
            && let Some(repo) = Repo::open(start.root.path())
        {
            for peer in &start.peers {
                match repo.cached_branches(&peer.id) {
                    Ok(tips) if !tips.is_empty() => {
                        peer_branches.insert(peer.id, tips);
                    }
                    Ok(_) => {}
                    Err(e) => log::warn!("cached Git branch tips could not be read: {e:#}"),
                }
            }
        }

        Ok(Arc::new(Self {
            bootstrap_paths: RwLock::new(
                start
                    .root
                    .path()
                    .join(".git/beep-recovery")
                    .exists()
                    .then(HashSet::new),
            ),
            recovery: Arc::default(),
            recovery_turn: Mutex::default(),
            name: start.name,
            key: start.key,
            root: start.root,
            ignored,
            file: start.file,
            replica,
            state: Mutex::new(state),
            rules: Arc::new(RwLock::new(rules)),
            rules_text: start.rules,
            seq: watch::Sender::new(seq),
            saved_seq: watch::Sender::new(seq),
            status: watch::Sender::new(status),
            peers: watch::Sender::new(start.peers),
            merged: Notify::new(),
            touched: Notify::new(),
            forced: Notify::new(),
            matching: Notify::new(),
            peer_commits: watch::Sender::new(0),
            rescans: watch::Sender::new(0),
            paused: watch::Sender::new(start.paused),
            stop: CancellationToken::new(),
            unheard: Mutex::new(unheard),
            caught: Mutex::default(),
            peer_heads: Mutex::default(),
            peer_branches: Mutex::new(peer_branches),
            applied: Mutex::new(Instant::now()),
            pull_again: watch::Sender::new(0),
        }))
    }

    /// Blocks only inside the blocking pool, keeping Git's index stable for the write.
    pub(super) fn write_guard(&self, checkout: Option<Checkout>) -> Result<WriteGuard> {
        match &self.ignored {
            Some(scope) => scope.guard(),
            None => WriteGuard::acquire(self.root(), checkout),
        }
    }

    pub(super) async fn check_ignored(&self) -> Result<()> {
        if let Some(scope) = self.ignored.clone() {
            spawn_blocking(move || scope.check()).await??;
        }

        Ok(())
    }

    pub(super) fn lock(&self) -> MutexGuard<'_, State> {
        crate::lock(&self.state)
    }

    pub(in crate::files::sync) fn set_recovery(
        &self,
        holds: Option<crate::files::holds::Recoveries>,
    ) {
        let changed = {
            let mut current = crate::lock(&self.recovery);
            let changed = current.is_some() != holds.is_some();
            *current = holds;

            if changed {
                // What was asked belongs to the opt-in that asked it.
                *crate::lock(&self.recovery_turn) = RecoveryTurn::Idle;
            }

            changed
        };

        if changed {
            if crate::lock(&self.recovery).is_some() {
                *crate::write_lock(&self.bootstrap_paths) = Some(HashSet::new());
            }

            self.forced.notify_one();
        }
    }

    pub(in crate::files::sync) fn name(&self) -> &str {
        &self.name
    }

    pub(in crate::files::sync) fn root(&self) -> &Path {
        self.root.path()
    }

    pub(in crate::files::sync) fn key(&self) -> Key {
        self.key
    }

    /// Where its state is kept.
    pub(in crate::files::sync) fn file(&self) -> &Path {
        &self.file
    }

    pub(in crate::files::sync) fn replica(&self) -> Replica {
        self.replica
    }

    pub(in crate::files::sync) fn subscribe(&self) -> watch::Receiver<u64> {
        self.seq.subscribe()
    }

    pub(in crate::files::sync) fn watch_saved(&self) -> watch::Receiver<u64> {
        self.saved_seq.subscribe()
    }

    pub(in crate::files::sync) fn status(&self) -> ProjectStatus {
        watched::read_sent(&self.status, Clone::clone)
    }

    pub(in crate::files::sync) fn watch_status(&self) -> watch::Receiver<ProjectStatus> {
        self.status.subscribe()
    }

    pub(in crate::files::sync) fn has_peer(&self, peer: &EndpointId) -> bool {
        watched::read_sent(&self.peers, |peers| {
            peers.iter().any(|addr| addr.id == *peer)
        })
    }

    pub(in crate::files::sync) fn watch_peers(&self) -> watch::Receiver<Vec<EndpointAddr>> {
        self.peers.subscribe()
    }

    pub(in crate::files::sync) fn peer_ids(&self) -> Vec<EndpointId> {
        watched::read_sent(&self.peers, |peers| {
            peers.iter().map(|addr| addr.id).collect()
        })
    }

    pub(in crate::files::sync) fn set_peers(&self, peers: Vec<EndpointAddr>) {
        self.peers.send_if_modified(|now| {
            let changed = *now != peers;
            *now = peers;
            changed
        });
    }

    /// Whether this node holds `hash` of the project, to serve it.
    pub(in crate::files::sync) fn has(&self, hash: &Hash) -> bool {
        self.lock().have.contains_key(hash.as_bytes())
    }

    /// The last change after `since`, if there is one.
    pub(in crate::files::sync) fn last_change_after(&self, since: u64) -> Option<u64> {
        Some(self.lock().saved.map.last_change()).filter(|&last| last > since)
    }

    /// The pulls start over: every peer has to be heard again.
    pub(super) fn listen_again(&self) {
        *crate::lock(&self.unheard) = self.peer_ids().into_iter().collect();
    }

    /// `peer` sent what it had, or could not be asked.
    pub(in crate::files::sync) fn heard(&self, peer: &EndpointId) {
        if crate::lock(&self.unheard).remove(peer) {
            // Publishes the status again.
            self.merged.notify_one();
        }
    }

    /// `peer` sent all it had, and a pull that went through brought it.
    pub(in crate::files::sync) fn caught_up(&self, peer: &EndpointId) {
        crate::lock(&self.caught).insert(*peer, Instant::now());
        // Publishes the status again, for whoever waits on it.
        self.merged.notify_one();
    }

    /// A round that began at `began` is done.
    pub(super) fn applied_from(&self, began: Instant) {
        *crate::lock(&self.applied) = began;
        // Whoever waits for a round after a pull looks again.
        self.status.send_modify(|_| {});
    }

    /// Whether a pull that went through brought all `peer` had after
    /// `since`, and a round of bringing the folder in line has come after
    /// it. A peer that is out of reach never does. What the round could
    /// not do yet is in the status.
    pub(in crate::files::sync) fn caught_up_since(
        &self,
        peer: &EndpointId,
        since: Instant,
    ) -> bool {
        let caught = crate::lock(&self.caught).get(peer).copied();
        caught.is_some_and(|at| at >= since && *crate::lock(&self.applied) >= at)
    }

    /// Ends the pulls; each starts again and says what its peer has.
    pub(in crate::files::sync) fn pull_again(&self) {
        self.pull_again.send_modify(|count| *count += 1);
    }

    pub(in crate::files::sync) fn watch_pull_again(&self) -> watch::Receiver<u64> {
        self.pull_again.subscribe()
    }

    pub(super) fn heard_all(&self) -> bool {
        crate::lock(&self.unheard).is_empty()
    }

    pub(in crate::files::sync) fn changes(
        &self,
        since: u64,
        limit: usize,
        checkout: Checkout,
    ) -> Result<Changes> {
        let state = self.lock();

        if state.saved.checkout != Some(checkout) {
            bail!("the file list changed base before this batch was sent");
        }

        let map = &state.saved.map;

        // An unsaved edit can move a path past this page. If a crash restores
        // its old position, peers that got the page would skip the path.
        if map.seq() > watched::read_sent(&self.saved_seq, |saved| *saved) {
            return Ok(Changes {
                entries: Vec::new(),
                upto: since,
            });
        }

        Ok(map.since(since, limit))
    }

    pub(in crate::files::sync) fn may_export(&self, path: &str) -> bool {
        crate::read_lock(&self.bootstrap_paths)
            .as_ref()
            .is_none_or(|allowed| allowed.contains(path))
    }

    /// Takes the answer to the last ask, leaving the turn as it stood: an
    /// answer belongs to the round that takes it, and any other round clears it.
    /// Returns it with whether an ask is still waiting.
    pub(super) fn take_recovery_answer(
        &self,
    ) -> (Option<Result<crate::files::holds::RepositoryHold>>, bool) {
        let mut turn = crate::lock(&self.recovery_turn);

        match std::mem::take(&mut *turn) {
            RecoveryTurn::Answered(answer) => (Some(answer), false),
            RecoveryTurn::Asked => {
                *turn = RecoveryTurn::Asked;
                (None, true)
            }
            RecoveryTurn::Idle => (None, false),
        }
    }

    /// Publishes that local edits wait for recovery to be let run.
    pub(super) fn ask_recovery(&self, asks: &crate::files::holds::Recoveries) {
        *crate::lock(&self.recovery_turn) = RecoveryTurn::Asked;
        asks.ask(self.root());
    }

    /// Keeps the answer only for the ask that is waiting; any other answer
    /// is dropped, and its hold with it.
    pub(in crate::files::sync) fn answer_recovery(
        &self,
        answer: Result<crate::files::holds::RepositoryHold>,
    ) {
        let mut turn = crate::lock(&self.recovery_turn);

        // Paused, no round runs to use it: the answer and its hold go, and the
        // round after the pause asks afresh.
        if self.is_paused() {
            *turn = RecoveryTurn::Idle;
            return;
        }

        if matches!(*turn, RecoveryTurn::Asked) {
            *turn = RecoveryTurn::Answered(answer);
            drop(turn);
            self.forced.notify_one();
        }
    }

    pub(super) fn recovery_enabled(&self) -> bool {
        crate::lock(&self.recovery).is_some()
    }

    pub(in crate::files::sync) async fn stopped(&self) {
        self.stop.cancelled().await;
    }

    pub(in crate::files::sync) fn is_stopped(&self) -> bool {
        self.stop.is_cancelled()
    }

    pub(in crate::files::sync) fn stop(&self) {
        self.stop.cancel();
        // A stopped project recovers nothing: a waiting answer and its hold go now.
        *crate::lock(&self.recovery_turn) = RecoveryTurn::Idle;
    }

    pub(in crate::files::sync) fn cursor(&self, peer: &EndpointId) -> Cursor {
        self.lock()
            .saved
            .cursors
            .get(&peer.to_string())
            .copied()
            .unwrap_or_default()
    }

    /// The commit this checkout is at, even when Git files stay local.
    pub(in crate::files::sync) async fn head(&self) -> Result<Checkout> {
        let root = self.root().to_owned();
        let head = spawn_blocking(move || Checkout::read(&root)).await?;
        Ok(head)
    }

    /// Whether this folder still needs its first Git copy.
    pub(in crate::files::sync) fn copies_git(&self) -> bool {
        matches!(self.lock().saved.git_copy, Some(GitCopy::Pending { .. }))
            && crate::read_lock(&self.rules).syncs_git()
    }

    pub(in crate::files::sync) fn observe_head(
        &self,
        peer: &EndpointId,
        checkout: Checkout,
        branch: Option<String>,
    ) {
        let head = (checkout, branch);
        let before = crate::lock(&self.peer_heads).insert(*peer, head.clone());

        if before != Some(head) {
            self.peer_commits_changed();
        }
    }

    pub(in crate::files::sync) fn observe_branches(&self, peer: &EndpointId, tips: Vec<BranchTip>) {
        let before = crate::lock(&self.peer_branches).insert(*peer, tips.clone());

        if before != Some(tips) {
            self.peer_commits_changed();
        }
    }

    pub(in crate::files::sync) fn branch_aware(&self, peer: &EndpointId) -> bool {
        crate::lock(&self.peer_branches).contains_key(peer)
    }

    pub(in crate::files::sync) fn forget_branches(&self, peer: &EndpointId) {
        if crate::lock(&self.peer_branches).remove(peer).is_some() {
            self.peer_commits_changed();
        }
    }

    /// Wakes the task for a round: only a change in what peers advertise
    /// does, so a peer that reconnects with the same commits costs nothing.
    fn peer_commits_changed(&self) {
        self.peer_commits.send_modify(|count| *count += 1);
    }

    /// Checks the checkout before accepting a peer's files. Different
    /// commits wait for the user or an agent to push and pull through Git.
    pub(in crate::files::sync) async fn first_contact(
        &self,
        peer: &EndpointId,
        checkout: Checkout,
        branch: Option<String>,
    ) -> Result<()> {
        self.scan_before_matching().await?;
        let own = self.head().await?;
        self.observe_head(peer, checkout, branch);
        let copies_git = self.copies_git();
        let mut state = self.lock();
        if copies_git {
            if let Some(GitCopy::Pending { peer: donor }) = &mut state.saved.git_copy {
                if donor
                    .as_ref()
                    .is_some_and(|donor| donor != &peer.to_string())
                {
                    bail!("waiting for the first Git copy from another peer");
                }

                if checkout.present && donor.is_none() {
                    *donor = Some(peer.to_string());
                    state.unsaved = true;
                }
            }
        } else if own.differs(checkout) && !self.branch_aware(peer) {
            bail!("Git commits differ; push and pull through Git before syncing files");
        }

        if copies_git && checkout.present {
            state.saved.git_only = true;
        }
        state.saved.matched.insert(peer.to_string());
        state.unsaved = true;
        drop(state);
        self.merged.notify_one();
        Ok(())
    }

    /// Whether an established checkout must wait for Git before writing files.
    pub(in crate::files::sync) async fn check_git(&self) -> Result<()> {
        self.check_ignored().await?;

        if self.copies_git() {
            return Ok(());
        }

        let root = self.root().to_owned();
        let own = spawn_blocking(move || -> Result<Checkout> {
            if let Some(repo) = Repo::open(&root) {
                repo.check_recovery()?;
            }

            if gitdir::is_busy(&root) {
                bail!("{}", gitdir::busy_reason(&root));
            }

            Ok(Checkout::read(&root))
        })
        .await??;
        let aware: HashSet<_> = crate::lock(&self.peer_branches).keys().copied().collect();
        let heads = crate::lock(&self.peer_heads);

        if heads
            .iter()
            .any(|(peer, head)| self.has_peer(peer) && !aware.contains(peer) && own.differs(head.0))
        {
            bail!("Git commits differ; push and pull through Git before syncing files");
        }

        Ok(())
    }

    /// A list can only travel under the checkout its completed scan read.
    pub(in crate::files::sync) async fn list_checkout(&self) -> Result<Checkout> {
        self.check_ignored().await?;
        let now = self.head().await?;

        if self.lock().saved.checkout != Some(now) {
            bail!("waiting for a scan of the current Git checkout");
        }

        let root = self.root().to_owned();
        let busy =
            spawn_blocking(move || gitdir::is_busy(&root).then(|| gitdir::busy_reason(&root)))
                .await?;
        if let Some(reason) = busy {
            bail!("{reason}");
        }

        Ok(now)
    }

    /// The initial donor has sent its whole list successfully.
    pub(super) fn git_copy_received(&self) -> bool {
        let state = self.lock();

        match &state.saved.git_copy {
            Some(GitCopy::Pending { peer: Some(peer) }) => crate::lock(&self.caught)
                .keys()
                .any(|id| id.to_string() == *peer),
            _ => false,
        }
    }

    /// Brings the list in line with the folder before comparing or sending it.
    /// A list from before the user's last edits would count those as older.
    /// The task scans what the watcher reported, and the whole folder only
    /// when the watcher cannot vouch for it, so a peer that reconnects often
    /// does not cost a full scan each time.
    pub(in crate::files::sync) async fn scan_before_matching(&self) -> Result<()> {
        self.wait_for_scan(&self.matching, MATCH_SCAN).await?;
        self.check_ignored().await?;
        Ok(())
    }

    /// Merges what a peer sent, and where it got to.
    pub(in crate::files::sync) fn merge_from(
        &self,
        peer: &EndpointId,
        entries: &[Entry],
        cursor: Cursor,
    ) {
        let allowed_git = crate::read_lock(&self.rules).syncs_git();
        let mut state = self.lock();
        let copies_git =
            allowed_git && matches!(state.saved.git_copy, Some(GitCopy::Pending { .. }));
        if copies_git && let Some(GitCopy::Pending { peer: donor }) = &mut state.saved.git_copy {
            match donor {
                Some(donor) if donor != &peer.to_string() => return,
                None if entries.iter().any(|entry| gitdir::in_git(&entry.path)) => {
                    *donor = Some(peer.to_string());
                    state.unsaved = true;
                }
                _ => {}
            }
        }

        let entries: Vec<_> = entries
            .iter()
            .filter(|entry| {
                if copies_git
                    && matches!(
                        state.saved.git_copy,
                        Some(GitCopy::Pending { peer: Some(_) })
                    )
                {
                    gitdir::in_git(&entry.path)
                } else {
                    !self.root().join(gitdir::GIT).exists()
                        && !state.saved.git_only
                        && !gitdir::in_git(&entry.path)
                }
            })
            .cloned()
            .collect();
        let merged = merge_peer(&mut state.saved.map, &entries);

        if merged.refused > 0 {
            log::warn!(
                "{}: left out {} entries from {peer}: bad paths, or past {MAX_FILES} files",
                self.name,
                merged.refused
            );
        }

        let changed = merged.changed;
        state.saved.cursors.insert(peer.to_string(), cursor);
        state.unsaved = true;
        let seq = state.saved.map.seq();
        drop(state);

        if changed {
            self.seq.send_replace(seq);
        }

        self.merged.notify_one();
    }

    pub(super) fn publish(&self, update: impl FnOnce(&mut ProjectStatus)) {
        let changed = self.status.send_if_modified(|status| {
            let before = status.clone();
            update(status);
            *status != before
        });

        if changed {
            crate::events::system().changed(beep_proto::node::events::Topic::new("sync"));
        }
    }

    pub(in crate::files::sync) fn is_paused(&self) -> bool {
        watched::read_sent(&self.paused, |paused| *paused)
    }

    /// Pauses the project or resumes it; the task follows.
    pub(in crate::files::sync) fn set_paused(&self, paused: bool) {
        let changed = self.paused.send_if_modified(|now| {
            let changed = *now != paused;
            *now = paused;
            changed
        });

        if changed && paused {
            // No round runs while paused to take an answer: the resume asks again.
            *crate::lock(&self.recovery_turn) = RecoveryTurn::Idle;
        }
    }

    /// Scans the folder now and brings it in line with what the peers
    /// sent, then says how the project stands. Waits at most `limit`.
    pub(in crate::files::sync) async fn rescan(&self, limit: Duration) -> Result<ProjectStatus> {
        self.wait_for_scan(&self.forced, limit).await
    }

    /// Asks the task through `ask` to scan, and waits at most `limit`.
    async fn wait_for_scan(&self, ask: &Notify, limit: Duration) -> Result<ProjectStatus> {
        if self.is_paused() {
            bail!("sync of {} is paused; resume it to sync", self.name);
        }

        let mut done = self.rescans.subscribe();
        let before = watched::read_and_update(&mut done, |count| *count);
        ask.notify_one();
        timeout(limit, watched::wait_for(&mut done, |count| *count > before))
            .await
            .with_context(|| format!("{} did not finish scanning in time", self.name))?
            .context("the project stopped")?;
        Ok(self.status())
    }

    /// Settles the conflict copy at `path`: deletes it, or puts it in
    /// place of the file it stands beside. The folder changes on this
    /// machine only; the scan that follows syncs it.
    pub(in crate::files::sync) async fn resolve(&self, path: &str, keep: Keep) -> Result<()> {
        if self.is_paused() {
            bail!(
                "sync of {} is paused; resume it to settle conflicts",
                self.name
            );
        }

        if gitdir::in_git(path) && !self.copies_git() {
            bail!("resolve Git conflicts through Git on this machine");
        }

        if !valid_path(path) || !is_conflict(path) {
            bail!("{path} is not a conflict copy");
        }

        let original =
            original_of(path).with_context(|| format!("{path} is not a conflict copy"))?;
        if gitdir::in_git(path) {
            return self.resolve_ref(path, &original, keep).await;
        }

        let (copy, beside) = {
            let state = self.lock();
            let stat = |p: &str| state.saved.seen.get(p).map(|seen| seen.stat);
            (stat(path), stat(&original))
        };
        let copy = copy.with_context(|| format!("{path} is not on this machine yet"))?;
        let (root, path) = (self.root.clone(), path.to_owned());
        let settle_it = Settle { copy, beside, keep };
        let ignored = self.ignored.clone();
        spawn_blocking(move || {
            let _guard = ignored.as_ref().map(IgnoredRoot::guard).transpose()?;
            settle(&root, &path, &original, settle_it)
        })
        .await??;
        self.touched.notify_one();
        Ok(())
    }
}

impl Shared {
    /// Settles the ref that kept the losing side of two commits to
    /// `original`: drops it, or moves `original` to it. The working tree
    /// stays as it is, and `git status` says how it differs.
    async fn resolve_ref(&self, path: &str, original: &str, keep: Keep) -> Result<()> {
        let (copy, beside) = {
            let state = self.lock();
            let hash = |p: &str| state.saved.seen.get(p).map(|seen| Oid::from(seen.hash));
            (hash(path), hash(original))
        };
        let copy = copy.with_context(|| format!("{path} is not on this machine yet"))?;
        let root = self.root().to_owned();
        let (path, original) = (path.to_owned(), original.to_owned());
        let moved = spawn_blocking(move || -> Result<bool> {
            let repo = Repo::open(&root).context("the repository is not here")?;
            let moved = keep == Keep::Copy;

            if moved {
                repo.set_ref(gitdir::ref_name(&original), copy, beside)?;
            }

            repo.delete_ref(gitdir::ref_name(&path), copy)?;
            Ok(moved)
        })
        .await??;

        if moved {
            let mut state = self.lock();
            state.saved.index_stale = true;
            state.unsaved = true;
        }

        self.touched.notify_one();
        Ok(())
    }
}

/// What the scan last saw of a conflict copy and the file beside it, and
/// which to keep.
struct Settle {
    copy: Stat,
    beside: Option<Stat>,
    keep: Keep,
}

/// Settles conflict copy `path` of `original` on disk, if neither changed
/// since the last scan; blocks.
fn settle(root: &Folder, path: &str, original: &str, settle: Settle) -> Result<()> {
    const CHANGED: &str = "changed on this machine since it was scanned; try again in a moment";

    match settle.keep {
        Keep::Original => {
            if !disk::remove(root, path, &settle.copy)? {
                bail!("{path} {CHANGED}");
            }
        }
        Keep::Copy => {
            if let Some(beside) = settle.beside
                && !disk::remove(root, original, &beside)?
            {
                bail!("{original} {CHANGED}");
            }

            if disk::rename(root, path, original, &settle.copy)?.is_none() {
                bail!("{path} {CHANGED}");
            }
        }
    }

    Ok(())
}
