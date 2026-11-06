//! Temporary devices, such as containers, that went without saying so: every desktop removes
//! one that has been offline for `GONE_AFTER`. A device that stops cleanly asks to be removed
//! itself (`leave/1`); this is for the crash and the host that went away. Two desktops may both
//! remove the same device: the second finds it done.

use crate::error::Result;
use beep_mesh::{Device, Mesh};
use iroh::EndpointId;
use std::{
    collections::HashMap,
    hash::Hash,
    time::{Duration, Instant},
};

/// How long a temporary device is offline before a desktop removes it.
pub const GONE_AFTER: Duration = Duration::from_secs(10 * 60);

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
mod tests {
    use super::*;

    fn temporary(list: &[(u8, bool)]) -> HashMap<u8, bool> {
        list.iter().copied().collect()
    }

    #[test]
    fn a_temporary_device_offline_for_ten_minutes_is_due() {
        let mut leftovers = Leftovers::default();
        let start = Instant::now();
        let at = |secs| start + Duration::from_secs(secs);
        assert!(leftovers.step(&temporary(&[(1, true)]), at(0)).is_empty());
        assert!(leftovers.step(&temporary(&[(1, false)]), at(10)).is_empty());
        assert!(
            leftovers
                .step(&temporary(&[(1, false)]), at(609))
                .is_empty()
        );
        assert_eq!(leftovers.step(&temporary(&[(1, false)]), at(610)), [1]);
    }

    #[test]
    fn coming_back_starts_the_ten_minutes_over() {
        let mut leftovers = Leftovers::default();
        let start = Instant::now();
        let at = |secs| start + Duration::from_secs(secs);
        leftovers.step(&temporary(&[(1, false)]), at(0));
        assert!(leftovers.step(&temporary(&[(1, true)]), at(500)).is_empty());
        assert!(
            leftovers
                .step(&temporary(&[(1, false)]), at(700))
                .is_empty()
        );
        assert!(
            leftovers
                .step(&temporary(&[(1, false)]), at(1299))
                .is_empty()
        );
        assert_eq!(leftovers.step(&temporary(&[(1, false)]), at(1300)), [1]);
    }

    #[test]
    fn a_device_off_at_start_counts_from_then_and_one_gone_is_forgotten() {
        let mut leftovers = Leftovers::default();
        let start = Instant::now();
        let at = |secs| start + Duration::from_secs(secs);
        assert!(leftovers.step(&temporary(&[(1, false)]), at(0)).is_empty());
        // Removed by another desktop: no longer listed.
        assert!(leftovers.step(&temporary(&[]), at(300)).is_empty());
        // Listed again (a new device with the same key cannot be, but the list may lag).
        assert!(
            leftovers
                .step(&temporary(&[(1, false)]), at(600))
                .is_empty()
        );
        assert_eq!(leftovers.step(&temporary(&[(1, false)]), at(1200)), [1]);
    }
}
