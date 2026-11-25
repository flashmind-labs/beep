//! What a device serves as its mesh's gateway. The gateway loop is the only
//! writer; the asks, the apps and the board streams follow it.

use super::{board::Boards, cards::Teammates};
use beep_proto::{team::TeamRepo, watched::read_sent};
use iroh::EndpointId;
use std::collections::{BTreeMap, BTreeSet};
use tokio::sync::watch;

/// What this device serves as its mesh's gateway, as the gateway loop
/// publishes it (`ServedBoards::publish`): nobody else changes it.
#[derive(Default)]
pub(crate) struct Served {
    /// The machine of this mesh that acts for each team, as this device
    /// elected it.
    pub(crate) gateways: BTreeMap<TeamRepo, EndpointId>,
    /// The boards this device serves as its mesh's gateway.
    pub(crate) boards: Boards,
    /// The teammates' cards this device follows as its mesh's gateway.
    pub(crate) teammates: BTreeMap<TeamRepo, Teammates>,
}

/// The watch that holds `Served`. It knows nothing of the home or the events.
pub(super) struct ServedBoards(watch::Sender<Served>);

impl Default for ServedBoards {
    fn default() -> Self {
        Self(watch::channel(Served::default()).0)
    }
}

impl ServedBoards {
    /// `f` of what is served. Not inside another watch read.
    pub(super) fn read<R>(&self, f: impl FnOnce(&Served) -> R) -> R {
        read_sent(&self.0, f)
    }

    /// Forgets all that was served.
    pub(super) fn clear(&self) {
        self.0.send_replace(Served::default());
    }

    /// Changes what is served and returns the teams `me` acted for before or
    /// after `f`, whose cards changed.
    pub(super) fn publish(&self, me: EndpointId, f: impl FnOnce(&mut Served)) -> BTreeSet<TeamRepo> {
        let acting_in = |served: &Served| -> BTreeSet<TeamRepo> {
            served
                .gateways
                .iter()
                .filter(|(_, gateway)| **gateway == me)
                .map(|(repo, _)| repo.clone())
                .collect()
        };
        let mut acting = BTreeSet::new();
        self.0.send_modify(|served| {
            acting = acting_in(served);
            f(served);
            acting.extend(acting_in(served));
        });

        acting
    }
}
