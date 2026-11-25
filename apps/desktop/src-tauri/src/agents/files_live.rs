//! The reads of a folder that a window asks once and then follows. A window reads a folder's
//! listing, worktrees and the places of its sessions when it opens them; that read makes the
//! desktop remember what was asked. The node pushes a change to the folder on its `files`
//! topic, and the reads asked of it are made again and sent as views in an update of that
//! topic, so the window takes them from the push instead of asking again. `event_views` hears
//! the push, reserves the read here and sends the views.

use super::file_reads::{places_of, tree_of};
use crate::fleet::{
    Fleet,
    agents::PlaceAsk,
    asked::{BEFORE_WORKTREE_CREATORS, newer_than},
    view_signal::{Update, View, join_changed},
};
use beep_proto::{
    SessionId,
    node::{agent::Worktree, events::Topic},
};
use iroh::EndpointId;
use serde_json::json;
use std::{
    collections::{HashMap, HashSet, hash_map::Entry},
    sync::{Mutex, PoisonError},
};
use tauri::{AppHandle, Manager};

/// The most reads one folder keeps, the newest.
const KEPT: usize = 64;
/// The most sets of places one machine keeps, the newest.
const PLACES_KEPT: usize = 8;

/// A listing asked for, as the window passed it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct TreeRead {
    path: String,
    ignored: bool,
    deep: bool,
}

/// What is followed in one folder of one machine, each read with the window that asked it.
#[derive(Default)]
struct Folder {
    trees: Vec<(String, TreeRead)>,
    worktrees: Vec<(String, Option<SessionId>)>,
}

impl Folder {
    fn is_empty(&self) -> bool {
        self.trees.is_empty() && self.worktrees.is_empty()
    }
}

/// `items` without the window that asked each, each once.
fn distinct<T: PartialEq + Clone>(items: &[(String, T)]) -> Vec<T> {
    let mut out: Vec<T> = Vec::new();

    for (_, item) in items {
        if !out.contains(item) {
            out.push(item.clone());
        }
    }

    out
}

#[derive(Default)]
pub(crate) struct FilesLive(Mutex<Held>);

#[derive(Default)]
struct Held {
    folders: HashMap<(EndpointId, String), Folder>,
    places: HashMap<EndpointId, Vec<(String, Vec<PlaceAsk>)>>,
    /// The topics whose reads are being made now, and the update that came meanwhile, which
    /// they are made again for.
    reading: HashMap<(EndpointId, Topic), Option<Update>>,
}

impl Held {
    /// Drops the folders `followed` says no window follows, and the places of the machines left
    /// with no followed folder.
    fn keep(&mut self, followed: impl Fn(EndpointId, &str) -> bool) {
        self.folders
            .retain(|(machine, root), _| followed(*machine, root));
        let machines: HashSet<EndpointId> =
            self.folders.keys().map(|(machine, _)| *machine).collect();
        self.places.retain(|machine, _| machines.contains(machine));
    }

    /// Drops the reads of windows that are gone, and the folders and machines left with none.
    fn forget_windows(&mut self, windows: &HashSet<String>) {
        for folder in self.folders.values_mut() {
            folder.trees.retain(|(window, _)| windows.contains(window));
            folder
                .worktrees
                .retain(|(window, _)| windows.contains(window));
        }

        self.folders.retain(|_, folder| !folder.is_empty());

        for sets in self.places.values_mut() {
            sets.retain(|(window, _)| windows.contains(window));
        }

        self.places.retain(|_, sets| !sets.is_empty());
    }

    /// Whether the caller reads for `update` now: false when a read is running, which takes
    /// it up when it is done.
    fn begin(&mut self, key: (EndpointId, Topic), mut update: Update) -> bool {
        match self.reading.entry(key) {
            Entry::Occupied(mut running) => {
                // The update that waits stands for the one it replaces: it names both's paths.
                if let Some(earlier) = running.get() {
                    join_changed(earlier, &mut update);
                }

                running.insert(Some(update));
                false
            }
            Entry::Vacant(free) => {
                free.insert(None);
                true
            }
        }
    }

    /// The update that came while reading, if one did; else the reading ends.
    fn next(&mut self, key: &(EndpointId, Topic)) -> Option<Update> {
        let more = self.reading.get_mut(key).and_then(Option::take);

        if more.is_none() {
            self.reading.remove(key);
        }

        more
    }
}

/// Keeps `item` as the newest of `items`, at most `most` of them.
fn keep<T: PartialEq>(items: &mut Vec<T>, item: T, most: usize) {
    items.retain(|kept| *kept != item);
    items.push(item);

    if items.len() > most {
        items.remove(0);
    }
}

impl FilesLive {
    pub(crate) fn follow_tree(
        &self,
        window: &str,
        machine: EndpointId,
        root: &str,
        path: &str,
        ignored: bool,
        deep: bool,
    ) {
        let mut held = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let folder = held.folders.entry((machine, root.to_owned())).or_default();
        let read = TreeRead {
            path: path.to_owned(),
            ignored,
            deep,
        };
        keep(&mut folder.trees, (window.to_owned(), read), KEPT);
    }

    pub(crate) fn follow_worktrees(
        &self,
        window: &str,
        machine: EndpointId,
        folder: &str,
        session: Option<&SessionId>,
    ) {
        let mut held = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let at = held
            .folders
            .entry((machine, folder.to_owned()))
            .or_default();
        keep(
            &mut at.worktrees,
            (window.to_owned(), session.cloned()),
            KEPT,
        );
    }

    pub(crate) fn follow_places(&self, window: &str, machine: EndpointId, asks: &[PlaceAsk]) {
        let mut held = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let sets = held.places.entry(machine).or_default();
        keep(sets, (window.to_owned(), asks.to_vec()), PLACES_KEPT);
    }

    /// Ends what no window follows now: `followed` says whether a window follows a folder's
    /// `files` topic. A folder none follows is forgotten, and a machine's places with them when
    /// none of its folders is left.
    pub(crate) fn keep(
        &self,
        followed: impl Fn(EndpointId, &str) -> bool,
        windows: &HashSet<String>,
    ) {
        let mut held = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        held.forget_windows(windows);
        held.keep(followed);
    }

    /// Starts reading for `update` unless a read is running, which takes it up when it is done.
    /// `wanted` is whether a window follows the topic.
    pub(crate) fn reserve(&self, wanted: bool, machine: EndpointId, update: &Update) -> Reserve {
        let topic = &update.topic;
        let Some(root) = topic.key().filter(|_| !topic.is_pattern()) else {
            return Reserve::Unfollowed;
        };
        let mut held = self.0.lock().unwrap_or_else(PoisonError::into_inner);

        if !wanted {
            held.folders.remove(&(machine, root.to_owned()));
            return Reserve::Unfollowed;
        }

        if !held.folders.contains_key(&(machine, root.to_owned()))
            && !held.places.contains_key(&machine)
        {
            return Reserve::Unfollowed;
        }

        if held.begin((machine, topic.clone()), update.clone()) {
            Reserve::Start
        } else {
            Reserve::Busy
        }
    }

    /// What is followed for `topic`: the listings and worktrees of its folder, and the places
    /// of the machine.
    fn followed(
        &self,
        machine: EndpointId,
        topic: &Topic,
    ) -> (String, Followed, Vec<Vec<PlaceAsk>>) {
        let held = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let root = topic.key().unwrap_or_default().to_owned();
        let folder = held
            .folders
            .get(&(machine, root.clone()))
            .map(|folder| Followed {
                trees: distinct(&folder.trees),
                worktrees: distinct(&folder.worktrees),
            })
            .unwrap_or_default();
        let places = held
            .places
            .get(&machine)
            .map(|sets| distinct(sets))
            .unwrap_or_default();

        (root, folder, places)
    }

    /// The update that came while reading, if one did; else the reading ends.
    pub(crate) fn next(&self, machine: EndpointId, topic: &Topic) -> Option<Update> {
        let mut held = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        held.next(&(machine, topic.clone()))
    }
}

/// What the windows follow in a folder, each read once.
#[derive(Default)]
struct Followed {
    trees: Vec<TreeRead>,
    worktrees: Vec<Option<SessionId>>,
}

/// What `reserve` found.
pub(crate) enum Reserve {
    /// Nothing is followed in the folder: the update goes on as it is.
    Unfollowed,
    /// A read is running and takes the update up when it is done.
    Busy,
    Start,
}

/// Whether `machine`'s node says which sessions created each worktree: the released ones
/// do not.
fn lists_creators(fleet: &Fleet, machine: EndpointId) -> bool {
    machine == fleet.id() || names_creators(fleet.status().version(machine).as_deref())
}

fn names_creators(version: Option<&str>) -> bool {
    newer_than(version, &BEFORE_WORKTREE_CREATORS)
}

/// `list` as asked for `session`: the worktrees it created are marked.
fn created_by(list: &[Worktree], session: Option<&SessionId>) -> Vec<Worktree> {
    list.iter()
        .map(|tree| Worktree {
            created: session.is_some_and(|session| tree.created_by.contains(session)),
            created_by: Vec::new(),
            ..tree.clone()
        })
        .collect()
}

/// The views of everything followed for `topic`, each as the command it answers.
pub(crate) async fn read(app: &AppHandle, machine: EndpointId, topic: &Topic) -> Vec<View> {
    let fleet = app.state::<std::sync::Arc<Fleet>>();
    let (root, folder, places) = app.state::<FilesLive>().followed(machine, topic);
    let machine_id = machine.to_string();
    let trees = folder.trees.into_iter().map(|read| {
        let args = json!({
            "machineId": machine_id,
            "root": root,
            "path": read.path,
            "ignored": read.ignored,
            "deep": read.deep,
        });
        let (fleet, machine_id, root) = (&fleet, &machine_id, &root);
        async move {
            match tree_of(
                fleet,
                machine_id,
                root,
                &read.path,
                read.ignored,
                read.deep,
                true,
            )
            .await
            {
                Ok(tree) => View::new("agent_tree", args, tree),
                Err(error) => Some(View::failed("agent_tree", args, &error.to_string())),
            }
        }
    });
    // A node that says who created each worktree lists a folder once for every session.
    let listed = match folder.worktrees.is_empty() || !lists_creators(&fleet, machine) {
        true => None,
        false => Some(crate::fleet::agents::worktrees(&fleet, machine, &root, None).await),
    };
    let worktrees = folder.worktrees.into_iter().map(|session| {
        let args = json!({ "machineId": machine_id, "folder": root, "session": session });
        let (fleet, root, listed) = (&fleet, &root, &listed);
        async move {
            let answer = match listed {
                Some(Ok(list)) => Ok(created_by(list, session.as_ref())),
                Some(Err(error)) => Err(error.to_string()),
                None => crate::fleet::agents::worktrees(fleet, machine, root, session.as_ref())
                    .await
                    .map_err(|error| error.to_string()),
            };

            match answer {
                Ok(list) => View::new("agent_worktrees", args, list),
                Err(error) => Some(View::failed("agent_worktrees", args, &error)),
            }
        }
    });
    let places = places.into_iter().map(|asks| {
        let args = json!({ "machineId": machine_id, "asks": asks });
        let (fleet, machine_id) = (&fleet, &machine_id);
        async move {
            match places_of(fleet, machine_id, &asks).await {
                Ok(places) => View::new("agent_places", args, places),
                Err(error) => Some(View::failed("agent_places", args, &error.to_string())),
            }
        }
    });

    let (trees, worktrees, places) = tokio::join!(
        futures::future::join_all(trees),
        futures::future::join_all(worktrees),
        futures::future::join_all(places)
    );
    trees
        .into_iter()
        .chain(worktrees)
        .chain(places)
        .flatten()
        .collect::<Vec<View>>()
}

#[cfg(test)]
#[path = "../tests/agents_files_live.rs"]
mod tests;
