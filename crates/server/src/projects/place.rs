//! Which machine a project worker runs on: online, offering the agent, with a shared checkout
//! that has room.

use super::model::MAX_MACHINE_WORKERS;
use crate::{protocol::node::Capability, tools::machines::command_name};
use beep_projects::model::MachinePath;
use iroh::EndpointId;

/// What placement knows about one machine for one project.
#[derive(Debug, Clone)]
pub(crate) struct MachineFit {
    pub(crate) machine: EndpointId,
    pub(crate) name: String,
    pub(crate) online: bool,
    /// Whether the machine offers the project's coding agent.
    pub(crate) offers_agent: bool,
    pub(crate) checkout: Option<MachinePath>,
    /// Whether the checkout is shared with the project.
    pub(crate) shared: bool,
    pub(crate) capabilities: Vec<Capability>,
    /// Workers running there now.
    pub(crate) running: u32,
}

impl MachineFit {
    /// Why the machine cannot take a worker, or `None` when it can.
    fn unfit(&self) -> Option<Unfit> {
        if !self.online {
            Some(Unfit::Offline)
        } else if !self.offers_agent {
            Some(Unfit::NoAgent)
        } else if self.checkout.is_none() {
            Some(Unfit::NoCheckout)
        } else if !self.shared {
            Some(Unfit::NotShared)
        } else if i64::from(self.running) >= MAX_MACHINE_WORKERS {
            Some(Unfit::Full)
        } else {
            None
        }
    }

    fn covers(&self, needs: &[Capability]) -> bool {
        needs.iter().all(|need| self.capabilities.contains(need))
    }
}

/// What the caller asked for.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PlaceRequest<'a> {
    /// A machine the model named.
    pub(crate) named: Option<&'a str>,
    /// The machine the project is pinned to.
    pub(crate) pin: Option<&'a EndpointId>,
    pub(crate) needs: &'a [Capability],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unfit {
    Offline,
    NoAgent,
    NoCheckout,
    NotShared,
    Full,
}

impl Unfit {
    fn reason(self) -> &'static str {
        match self {
            Unfit::Offline => "it is offline",
            Unfit::NoAgent => "it does not offer the coding agent",
            Unfit::NoCheckout => "it has no checkout of the project",
            Unfit::NotShared => "its checkout is not shared with the project",
            Unfit::Full => "it is already running as many workers as it can",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum PlaceError {
    #[error("There is no machine called {0}.")]
    NoSuchMachine(String),
    #[error("{name} cannot take the worker: {}.", why.reason())]
    Unfit { name: String, why: Unfit },
    #[error(
        "{name} has what the task needs but no checkout of the project. \
         Ask the user to check the project out there first."
    )]
    NeedsCheckout { name: String },
    #[error("No machine with a checkout offers {}.", capability_list(.0))]
    NoneCovers(Vec<Capability>),
    #[error("Every machine that can take a worker is busy or unavailable.")]
    NoneFree,
}

fn capability_list(needs: &[Capability]) -> String {
    let names: Vec<&'static str> = needs.iter().map(|need| (*need).into()).collect();
    names.join(" and ")
}

/// The index in `fits` of the machine for the worker.
pub(crate) fn place(request: &PlaceRequest<'_>, fits: &[MachineFit]) -> Result<usize, PlaceError> {
    if let Some(named) = request.named {
        let wanted = command_name(named);
        let at = fits
            .iter()
            .position(|fit| command_name(&fit.name) == wanted)
            .ok_or_else(|| PlaceError::NoSuchMachine(named.to_owned()))?;
        return match fits[at].unfit() {
            None => Ok(at),
            Some(why) => Err(PlaceError::Unfit {
                name: fits[at].name.clone(),
                why,
            }),
        };
    }
    if let Some(pin) = request.pin
        && let Some(at) = fits
            .iter()
            .position(|fit| &fit.machine == pin && fit.unfit().is_none())
    {
        return Ok(at);
    }
    let best = fits
        .iter()
        .enumerate()
        .filter(|(_, fit)| fit.unfit().is_none() && fit.covers(request.needs))
        .min_by(|(_, a), (_, b)| (a.running, &a.name).cmp(&(b.running, &b.name)));
    if let Some((at, _)) = best {
        return Ok(at);
    }
    if request.needs.is_empty() {
        return Err(PlaceError::NoneFree);
    }
    // Nothing fits: say why the machines that cover the needs cannot take it.
    let covering: Vec<&MachineFit> = fits
        .iter()
        .filter(|fit| fit.covers(request.needs))
        .collect();
    if let Some(fit) = covering
        .iter()
        .find(|fit| fit.online && fit.offers_agent && fit.checkout.is_none())
    {
        return Err(PlaceError::NeedsCheckout {
            name: fit.name.clone(),
        });
    }
    if covering.is_empty() {
        return Err(PlaceError::NoneCovers(request.needs.to_vec()));
    }
    covering
        .iter()
        .find_map(|fit| match fit.unfit() {
            Some(Unfit::Full) | None => None,
            Some(why) => Some(PlaceError::Unfit {
                name: fit.name.clone(),
                why,
            }),
        })
        .map_or(Err(PlaceError::NoneFree), Err)
}

#[cfg(test)]
#[path = "../tests/projects_place.rs"]
mod tests;
