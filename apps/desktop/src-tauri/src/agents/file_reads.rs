//! The reads of a session's folder that a window follows: the listing of a folder and the places
//! its sessions work in. The commands ask for them and `files_live` makes them again on a push.

use crate::{
    error::Result,
    fleet::{
        Fleet,
        agents::{self as agent_nodes, PlaceAsk},
        files as direct, work_copy,
    },
};
use beep_mesh::machine::sessions::Reading;
use beep_proto::{GitChange, ListWorkTree, MAX_TREE_DEPTH, node::agent::WorkPlace};
use serde::Serialize;

/// One folder of the working folder, for the bar.
#[derive(Debug, Serialize)]
pub struct TreeFolder {
    path: String,
    entries: Vec<TreeEntry>,
    more: u32,
}

#[derive(Debug, Serialize)]
pub struct TreeEntry {
    name: String,
    folder: bool,
    size: u64,
    ignored: bool,
    /// `untracked`, `added`, `modified` or `conflicted`.
    change: Option<&'static str>,
}

fn change_name(change: GitChange) -> &'static str {
    match change {
        GitChange::Untracked {} => "untracked",
        GitChange::Added {} => "added",
        GitChange::Modified {} => "modified",
        GitChange::Conflicted {} => "conflicted",
    }
}

/// The folders `TREE_DEPTH` levels down from `path`, for the bar.
#[derive(Debug, Serialize)]
pub struct Tree {
    folders: Vec<TreeFolder>,
    truncated: bool,
}

/// How many levels one read of the tree takes.
const TREE_DEPTH: u32 = 3;

/// What `agent_tree` answers, as the folder's views push it too.
pub(crate) async fn tree_of(
    fleet: &Fleet,
    machine_id: &str,
    root: &str,
    path: &str,
    ignored: bool,
    deep: bool,
    fresh: bool,
) -> Result<Tree> {
    let ask = ListWorkTree {
        machine_id: machine_id.to_owned(),
        root: root.to_owned(),
        path: path.to_owned(),
        ignored,
        depth: if deep { MAX_TREE_DEPTH } else { TREE_DEPTH },
    };
    let cached = if fresh {
        None
    } else {
        work_copy::tree(fleet, &ask).await
    };
    let tree = match cached {
        Some(tree) => tree,
        None => {
            let machine = fleet.machine(&ask.machine_id)?;
            direct::tree(fleet, machine, &ask, Reading::Followed).await?
        }
    };
    let folders = tree
        .folders
        .into_iter()
        .map(|folder| TreeFolder {
            path: folder.path,
            entries: folder
                .entries
                .into_iter()
                .map(|e| TreeEntry {
                    name: e.name,
                    folder: e.folder,
                    size: e.size,
                    ignored: e.ignored,
                    change: e.change.map(change_name),
                })
                .collect(),
            more: folder.more,
        })
        .collect();
    Ok(Tree {
        folders,
        truncated: tree.truncated,
    })
}

/// Where sessions of one machine work, as their node gives it.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Places {
    /// Each asked session's checkout, in order; `None` for a node that does
    /// not say.
    Found { places: Vec<Option<WorkPlace>> },
    /// The machine did not answer yet, as while it starts or its link comes
    /// up: ask again later.
    Unreachable,
}

/// What `agent_places` answers, as a folder's views push it too.
pub(crate) async fn places_of(
    fleet: &Fleet,
    machine_id: &str,
    asks: &[PlaceAsk],
) -> Result<Places> {
    let Some(machine) = fleet.member(machine_id) else {
        let places = asks.iter().map(|_| None).collect();
        return Ok(Places::Found { places });
    };

    match agent_nodes::places(fleet, machine, asks).await {
        Ok(places) => Ok(Places::Found { places }),
        Err(e) if e.is_transport() => {
            log::debug!("session places not read yet; machine={machine_id} error={e}");
            Ok(Places::Unreachable)
        }
        Err(e) => Err(e),
    }
}
