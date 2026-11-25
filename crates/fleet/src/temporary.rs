//! Temporary devices, such as containers: every desktop removes one that has been offline for
//! `GONE_AFTER`, long enough that a restart or a reboot comes back first. A removed key never
//! rejoins. Two desktops may both remove the same device: the second finds it done.

use crate::error::Result;
use beep_mesh::{Device, Mesh};
use iroh::EndpointId;
use std::{
    collections::HashMap,
    hash::Hash,
    time::{Duration, Instant},
};

/// How long a temporary device is offline before a desktop removes it.
pub const GONE_AFTER: Duration = Duration::from_secs(24 * 60 * 60);

/// When each temporary device was first seen offline, by this desktop in this run. One that
/// was already off when the app started counts from then.
pub struct Leftovers<K> {
    since: HashMap<K, Instant>,
}

impl<K> Default for Leftovers<K> {
    fn default() -> Self {
        Self {
            since: HashMap::new(),
        }
    }
}

impl<K: Eq + Hash + Copy> Leftovers<K> {
    /// The devices offline for `GONE_AFTER` as of `now`, given each temporary member and
    /// whether it is online. A device that comes back starts over.
    pub fn step(&mut self, temporary: &HashMap<K, bool>, now: Instant) -> Vec<K> {
        self.since.retain(|id, _| temporary.get(id) == Some(&false));
        temporary
            .iter()
            .filter(|(_, online)| !**online)
            .filter_map(|(&id, _)| {
                let since = *self.since.entry(id).or_insert(now);
                (now.saturating_duration_since(since) >= GONE_AFTER).then_some(id)
            })
            .collect()
    }
}

impl<K> Leftovers<K> {
    /// When the first device seen offline is due, for the caller to wake at.
    pub fn due(&self) -> Option<Instant> {
        self.since.values().map(|since| *since + GONE_AFTER).min()
    }
}

/// Removes the temporary devices of `mesh` offline for `GONE_AFTER` as of `now`. Returns the
/// ones this desktop removed; one another desktop removed first is left out.
pub async fn sweep(
    mesh: &Mesh,
    leftovers: &mut Leftovers<EndpointId>,
    now: Instant,
) -> Result<Vec<Device>> {
    let temporaries = mesh.temporaries();
    let me = mesh.me();
    let devices = mesh.devices();
    let online: HashMap<EndpointId, bool> = devices
        .iter()
        .filter(|status| status.device.id != me && temporaries.contains(&status.device.id))
        .map(|status| (status.device.id, status.online))
        .collect();
    let mut removed = Vec::new();

    for id in leftovers.step(&online, now) {
        if mesh.remove_temporary(id).await?
            && let Some(status) = devices.iter().find(|status| status.device.id == id)
        {
            removed.push(status.device.clone());
        }
    }

    Ok(removed)
}

#[cfg(test)]
#[path = "tests/temporary.rs"]
mod tests;
