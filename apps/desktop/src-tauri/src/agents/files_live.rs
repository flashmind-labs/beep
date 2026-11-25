//! The reads of a folder that a window asks once and then follows. A window reads a folder's
//! listing, worktrees and the places of its sessions when it opens them; that read makes the
//! desktop remember what was asked. The node pushes a change to the folder on its `files`
//! topic, and the reads asked of it are made again and sent as views in an update of that
//! topic, so the window takes them from the push instead of asking again.

use super::{diff::places_of, files::tree_of};
use crate::fleet::{Fleet, agents::PlaceAsk, event_views::View, events::{Events, Update}};
use beep_proto::{SessionId, node::events::Topic};
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
#[derive(Clone, PartialEq, Eq)]
struct TreeRead {
    path: String,
    ignored: bool,
    deep: bool,
}

/// What is followed in one folder of one machine.
#[derive(Default)]
struct Folder {
    trees: Vec<TreeRead>,
    worktrees: Vec<Option<SessionId>>,
}

#[derive(Default)]
pub(crate) struct FilesLive(Mutex<Held>);

#[derive(Default)]
struct Held {
    folders: HashMap<(EndpointId, String), Folder>,
    places: HashMap<EndpointId, Vec<Vec<PlaceAsk>>>,
    /// The topics whose reads are being made now, and the update that came meanwhile, which
    /// they are made again for.
    reading: HashMap<(EndpointId, Topic), Option<Update>>,
}

impl Held {
    /// Drops the folders `followed` says no window follows, and the places of the machines left
    /// with no followed folder.
    fn keep(&mut self, followed: impl Fn(EndpointId, &str) -> bool) {
        self.folders.retain(|(machine, root), _| followed(*machine, root));
        let machines: HashSet<EndpointId> = self.folders.keys().map(|(machine, _)| *machine).collect();
        self.places.retain(|machine, _| machines.contains(machine));
    }

    /// Whether the caller reads for `update` now: false when a read is running, which takes
    /// it up when it is done.
    fn begin(&mut self, key: (EndpointId, Topic), update: Update) -> bool {
        match self.reading.entry(key) {
            Entry::Occupied(mut running) => {
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
        keep(&mut folder.trees, read, KEPT);
    }

    pub(crate) fn follow_worktrees(
        &self,
        machine: EndpointId,
        folder: &str,
        session: Option<&SessionId>,
    ) {
        let mut held = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let at = held
            .folders
            .entry((machine, folder.to_owned()))
            .or_default();
        keep(&mut at.worktrees, session.cloned(), KEPT);
    }

    pub(crate) fn follow_places(&self, machine: EndpointId, asks: &[PlaceAsk]) {
        let mut held = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let sets = held.places.entry(machine).or_default();
        keep(sets, asks.to_vec(), PLACES_KEPT);
    }

    /// Ends what no window follows now: `wanted` is each machine's node topics, as the windows'
    /// subscriptions leave them. A folder whose `files` topic is gone is forgotten, and a
    /// machine's places with them when none of its `files` topics is left.
    pub(crate) fn keep(&self, wanted: &std::collections::BTreeMap<EndpointId, Vec<Topic>>) {
        let mut held = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        held.keep(|machine, root| {
            wanted.get(&machine).is_some_and(|topics| {
                let topic = Topic::keyed("files", root);
                topics.iter().any(|wanted| wanted.matches(&topic))
            })
        });
    }

    /// Starts reading for `update` unless a read is running, which takes it up when it is done.
    fn reserve(&self, app: &AppHandle, machine: EndpointId, update: &Update) -> Reserve {
        let topic = &update.topic;
        let Some(root) = topic.key().filter(|_| !topic.is_pattern()) else {
            return Reserve::Unfollowed;
        };
        let wanted = app.state::<Events>().node_wanted(machine, topic);
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
    fn followed(&self, machine: EndpointId, topic: &Topic) -> (String, Folder, Vec<Vec<PlaceAsk>>) {
        let held = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let root = topic.key().unwrap_or_default().to_owned();
        let folder = held
            .folders
            .get(&(machine, root.clone()))
            .map(|folder| Folder {
                trees: folder.trees.clone(),
                worktrees: folder.worktrees.clone(),
            })
            .unwrap_or_default();

        (
            root,
            folder,
            held.places.get(&machine).cloned().unwrap_or_default(),
        )
    }

    /// The update that came while reading, if one did; else the reading ends.
    fn next(&self, machine: EndpointId, topic: &Topic) -> Option<Update> {
        let mut held = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        held.next(&(machine, topic.clone()))
    }
}

/// What `reserve` found.
enum Reserve {
    /// Nothing is followed in the folder: the update goes on as it is.
    Unfollowed,
    /// A read is running and takes the update up when it is done.
    Busy,
    Start,
}

/// The node said something on `topic`, a folder's `files`: what is followed there is read
/// again, and the windows that follow the topic are sent it as views. Returns true when the
/// windows are sent the update that way and the caller sends nothing: its readers take the
/// views, so they have nothing to read again. An initial update is the caller's to send.
pub(crate) fn heard(app: &AppHandle, machine: EndpointId, update: &Update) -> bool {
    let taken = !update.initial;

    match app.state::<FilesLive>().reserve(app, machine, update) {
        Reserve::Unfollowed => return false,
        Reserve::Busy => return taken,
        Reserve::Start => {}
    }

    let (app, mut update) = (app.clone(), update.clone());
    tauri::async_runtime::spawn(async move {
        loop {
            let views = read(&app, machine, &update.topic).await;

            if !views.is_empty() {
                app.state::<Events>().push_node_views(
                    machine,
                    update.topic.clone(),
                    update.revision,
                    json!({ "views": views }),
                );
            } else if !update.initial {
                // Nothing to read: the windows still hear that the folder changed.
                app.state::<Events>().dispatch_node(update.clone());
            }

            match app.state::<FilesLive>().next(machine, &update.topic) {
                Some(next) => update = next,
                None => return,
            }
        }
    });

    taken
}

/// The views of everything followed for `topic`, each as the command it answers.
async fn read(app: &AppHandle, machine: EndpointId, topic: &Topic) -> Vec<View> {
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
    let worktrees = folder.worktrees.into_iter().map(|session| {
        let args = json!({ "machineId": machine_id, "folder": root, "session": session });
        let (fleet, root) = (&fleet, &root);
        async move {
            match crate::fleet::agents::worktrees(fleet, machine, root, session.as_ref()).await {
                Ok(list) => View::new("agent_worktrees", args, list),
                Err(error) => Some(View::failed("agent_worktrees", args, &error.to_string())),
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
