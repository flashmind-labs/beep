//! Which machine a project worker runs on: online, offering the agent, with a shared checkout
//! that has room.

use super::model::{MAX_MACHINE_WORKERS, MachinePath};
use crate::protocol::node::Capability;
use crate::tools::machines::command_name;
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
mod tests {
    use super::*;
    use iroh::SecretKey;

    fn fit(n: u8, name: &str, capabilities: &[Capability]) -> MachineFit {
        MachineFit {
            machine: SecretKey::from_bytes(&[n; 32]).public(),
            name: name.to_owned(),
            online: true,
            offers_agent: true,
            checkout: Some(MachinePath::parse("/srv/project").expect("a valid path")),
            shared: true,
            capabilities: capabilities.to_vec(),
            running: 0,
        }
    }

    fn any() -> PlaceRequest<'static> {
        PlaceRequest {
            named: None,
            pin: None,
            needs: &[],
        }
    }

    #[test]
    fn named_machine_wins() {
        let fits = [fit(1, "build-box", &[]), fit(2, "gpu-box", &[])];
        let request = PlaceRequest {
            named: Some("GPU box"),
            ..any()
        };
        assert_eq!(place(&request, &fits), Ok(1));
    }

    #[test]
    fn named_machine_offline_says_why() {
        let mut gpu = fit(2, "gpu-box", &[]);
        gpu.online = false;
        let fits = [fit(1, "build-box", &[]), gpu];
        let request = PlaceRequest {
            named: Some("gpu-box"),
            ..any()
        };
        assert_eq!(
            place(&request, &fits),
            Err(PlaceError::Unfit {
                name: "gpu-box".into(),
                why: Unfit::Offline
            })
        );
        let request = PlaceRequest {
            named: Some("nas-box"),
            ..any()
        };
        assert_eq!(
            place(&request, &fits),
            Err(PlaceError::NoSuchMachine("nas-box".into()))
        );
    }

    #[test]
    fn pin_beats_capabilities() {
        let fits = [
            fit(1, "build-box", &[]),
            fit(2, "gpu-box", &[Capability::Cuda]),
        ];
        let request = PlaceRequest {
            pin: Some(&fits[0].machine),
            needs: &[Capability::Cuda],
            ..any()
        };
        assert_eq!(place(&request, &fits), Ok(0));
    }

    #[test]
    fn capabilities_pick_gpu_box() {
        let fits = [
            fit(1, "build-box", &[]),
            fit(2, "gpu-box", &[Capability::Cuda]),
        ];
        let request = PlaceRequest {
            needs: &[Capability::Cuda],
            ..any()
        };
        assert_eq!(place(&request, &fits), Ok(1));
        assert_eq!(
            place(
                &PlaceRequest {
                    needs: &[Capability::Xcode],
                    ..any()
                },
                &fits
            ),
            Err(PlaceError::NoneCovers(vec![Capability::Xcode]))
        );
    }

    #[test]
    fn never_a_machine_without_checkout() {
        let mut gpu = fit(2, "gpu-box", &[Capability::Cuda]);
        gpu.checkout = None;
        let fits = [fit(1, "build-box", &[]), gpu];
        let request = PlaceRequest {
            needs: &[Capability::Cuda],
            ..any()
        };
        assert_eq!(
            place(&request, &fits),
            Err(PlaceError::NeedsCheckout {
                name: "gpu-box".into()
            })
        );
    }

    #[test]
    fn offline_machine_with_the_capability_is_named() {
        let mut gpu = fit(2, "gpu-box", &[Capability::Cuda]);
        gpu.online = false;
        let fits = [fit(1, "build-box", &[]), gpu];
        let request = PlaceRequest {
            needs: &[Capability::Cuda],
            ..any()
        };
        assert_eq!(
            place(&request, &fits),
            Err(PlaceError::Unfit {
                name: "gpu-box".into(),
                why: Unfit::Offline
            })
        );
    }

    #[test]
    fn unshared_checkout_is_skipped() {
        let mut first = fit(1, "build-box", &[]);
        first.shared = false;
        let fits = [first, fit(2, "gpu-box", &[])];
        assert_eq!(place(&any(), &fits), Ok(1));
    }

    #[test]
    fn fewest_running_breaks_ties() {
        let mut busy = fit(1, "a-box", &[]);
        busy.running = 1;
        let fits = [busy, fit(2, "c-box", &[]), fit(3, "b-box", &[])];
        assert_eq!(place(&any(), &fits), Ok(2));
    }

    #[test]
    fn full_machines_are_skipped() {
        let mut full = fit(1, "a-box", &[]);
        full.running = 2;
        let mut fits = [full, fit(2, "b-box", &[])];
        assert_eq!(place(&any(), &fits), Ok(1));
        fits[1].running = 2;
        assert_eq!(place(&any(), &fits), Err(PlaceError::NoneFree));
    }
}
