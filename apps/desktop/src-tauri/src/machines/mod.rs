//! The account's machines: listing, adding and updating them, their
//! terminals, emulators and forwarded ports, and this Mac as a node (`this_mac`, `desktop`, `mcp_command`).

pub(crate) mod add;
pub(crate) mod desktop;
pub(crate) mod devices;
pub(crate) mod forwards;
pub(crate) mod list;
#[cfg(desktop)]
pub(crate) mod mcp_command;
pub(crate) mod mcp_servers;
#[cfg(desktop)]
pub(crate) mod node_updates;
pub(crate) mod sign_in;
pub(crate) mod terminals;
#[cfg(desktop)]
pub(crate) mod this_mac;
#[cfg(mobile)]
#[path = "phone.rs"]
pub(crate) mod this_mac;
pub(crate) mod tools;

use crate::error::{DesktopError, Result};
use crate::store::lock;
use beep_proto::node::desktop::Permissions;
use serde::Serialize;
use std::{
    collections::HashMap,
    hash::Hash,
    sync::{Mutex, MutexGuard},
};

/// Whether this Mac is lent to the mesh, as Settings shows it. A phone is always `Off`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ThisMacStatus {
    Off,
    #[cfg_attr(mobile, allow(dead_code))]
    Connected,
}

/// What Settings shows under This Mac, and what the command center needs
/// to tell this Mac apart in the machine list.
#[derive(Debug, Serialize)]
pub struct ThisMacView {
    /// This Mac's key, its id in the mesh.
    id: String,
    /// The name this Mac has in the mesh.
    name: String,
    enabled: bool,
    status: ThisMacStatus,
    permissions: Permissions,
}

/// What the windows follow, by key, each with where its input goes.
pub(crate) struct Registry<K, S>(Mutex<Following<K, S>>);

struct Following<K, S> {
    next: u64,
    by_key: HashMap<K, Follow<S>>,
}

struct Follow<S> {
    /// Which attach this is, so an older one ending leaves a newer alone.
    serial: u64,
    /// The window the key is in; closing it ends the follow.
    window: String,
    up: S,
}

impl<K, S> Default for Registry<K, S> {
    fn default() -> Self {
        Self(Mutex::new(Following {
            next: 0,
            by_key: HashMap::new(),
        }))
    }
}

impl<K: Eq + Hash, S: Clone> Registry<K, S> {
    fn lock(&self) -> MutexGuard<'_, Following<K, S>> {
        lock(&self.0)
    }

    pub(crate) fn follow(&self, key: K, window: &str, up: S) -> u64 {
        let mut following = self.lock();
        following.next += 1;
        let serial = following.next;
        let follow = Follow {
            serial,
            window: window.to_owned(),
            up,
        };
        following.by_key.insert(key, follow);
        serial
    }

    /// Window `label` closed: what it follows stops, which ends the streams.
    pub(crate) fn clear(&self, label: &str) {
        self.lock()
            .by_key
            .retain(|_, follow| follow.window != label);
    }

    pub(crate) fn unfollow(&self, key: &K, serial: u64) {
        let mut following = self.lock();

        if following
            .by_key
            .get(key)
            .is_some_and(|f| f.serial == serial)
        {
            following.by_key.remove(key);
        }
    }

    /// Drops `key`'s sender, which finishes its stream.
    pub(crate) fn detach(&self, key: &K) {
        self.lock().by_key.remove(key);
    }

    /// Where `key`'s input goes; `what` names it in the error.
    pub(crate) fn get(&self, key: &K, what: &str) -> Result<S> {
        self.lock()
            .by_key
            .get(key)
            .map(|follow| follow.up.clone())
            .ok_or_else(|| DesktopError::BadRequest(format!("The {what} is not attached.")))
    }
}

/// Forgets an attach once its stream ends, however it ends.
pub(crate) struct Unfollow<'a, K: Eq + Hash, S: Clone> {
    pub(crate) registry: &'a Registry<K, S>,
    pub(crate) key: K,
    pub(crate) serial: u64,
}

impl<K: Eq + Hash, S: Clone> Drop for Unfollow<'_, K, S> {
    fn drop(&mut self) {
        self.registry.unfollow(&self.key, self.serial);
    }
}
