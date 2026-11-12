//! Remembering issue hosts a machine could not reach, so their reads stop for a while instead of
//! each starting the host's CLI again.

use super::{HostExit, HostTimeout, IssueSource};
use crate::model::IssueHost;
use anyhow::Result;
use iroh::EndpointId;
use std::{
    collections::HashMap,
    future::Future,
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};
use thiserror::Error;
use tokio::{sync::watch, time::Instant};

/// How long a host is left alone after its first failure to answer.
pub const FIRST_PAUSE: Duration = Duration::from_secs(60);
/// The longest a host is left alone; each failure in a row doubles the pause up to it.
pub const LONGEST_PAUSE: Duration = Duration::from_secs(15 * 60);

/// What a host CLI prints when it got no connection to the host: Go's resolver and dialer, as
/// teacli and gh print them, and gh's own summary.
const NO_CONNECTION: &[&str] = &[
    "no such host",
    "connection refused",
    "i/o timeout",
    "network is unreachable",
    "no route to host",
    "server misbehaving",
    "temporary failure in name resolution",
    "could not resolve host",
    "error connecting to",
    "dial tcp",
];

/// A read that did not reach the host, now or when it was last tried.
#[derive(Debug, Clone, Error)]
#[error("{0}")]
pub struct Unreachable(String);

/// Whether `error` says the host could not be reached.
pub fn is_unreachable(error: &anyhow::Error) -> bool {
    error.downcast_ref::<Unreachable>().is_some()
        || matches!(Reach::of_error(error), Reach::None(_))
}

/// For each machine and host, whether the host answered when last asked. A host the machine
/// could not reach is not asked again there for a pause, which doubles with each failure in a
/// row, and while it is unknown one read asks it and the others wait for that one.
#[derive(Debug, Default)]
pub struct HostGates {
    gates: Mutex<HashMap<HostOnMachine, Arc<watch::Sender<Gate>>>>,
}

/// A host as one machine reaches it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct HostOnMachine {
    machine: EndpointId,
    host: IssueHost,
}

impl HostGates {
    /// Runs `read` against `source`'s host unless the host is paused on its machine, which gives
    /// the remembered [`Unreachable`] at once.
    pub async fn run<F>(&self, source: &IssueSource, read: impl Fn() -> F) -> Result<String>
    where
        F: Future<Output = Result<String>>,
    {
        let gate = self.gate(source);

        loop {
            let pass = match enter(&gate, Instant::now()) {
                Enter::Pass(pass) => pass,
                Enter::Paused(error) => return Err(Unreachable(error).into()),
                Enter::Wait => {
                    let mut changed = gate.subscribe();
                    // The sender is held here, so the channel stays open.
                    let _ = changed.wait_for(|gate| !gate.is_probing()).await;
                    continue;
                }
            };
            let result = read().await;
            let reach = Reach::of(&result);
            let result = match &reach {
                Reach::None(error) => Err(Unreachable(error.clone()).into()),
                _ => result,
            };

            pass.settle(reach, Instant::now());
            return result;
        }
    }

    fn gate(&self, source: &IssueSource) -> Arc<watch::Sender<Gate>> {
        let mut gates = self.gates.lock().unwrap_or_else(PoisonError::into_inner);
        gates
            .entry(HostOnMachine {
                machine: source.machine,
                host: source.host.clone(),
            })
            .or_insert_with(|| Arc::new(watch::Sender::new(Gate::Untried)))
            .clone()
    }
}

/// What is known about one host on one machine.
#[derive(Debug, Clone)]
enum Gate {
    /// Not asked yet, or the last read said nothing about the host.
    Untried,
    /// It answered when last asked; reads go ahead together.
    Open,
    /// One read is asking; `paused` is the pause it follows, if any.
    Probing {
        paused: Option<Paused>,
    },
    Paused(Paused),
}

impl Gate {
    fn is_probing(&self) -> bool {
        matches!(self, Gate::Probing { .. })
    }
}

#[derive(Debug, Clone)]
struct Paused {
    until: Instant,
    failures: u32,
    error: String,
}

impl Paused {
    fn after(failures: u32, error: String, now: Instant) -> Self {
        Self {
            until: now + pause(failures),
            failures,
            error,
        }
    }
}

/// How long a host is left alone after `failures` failures in a row.
fn pause(failures: u32) -> Duration {
    let doublings = failures.saturating_sub(1).min(8);
    FIRST_PAUSE
        .saturating_mul(1 << doublings)
        .min(LONGEST_PAUSE)
}

enum Enter<'g> {
    Pass(Pass<'g>),
    Paused(String),
    Wait,
}

fn enter(gate: &watch::Sender<Gate>, now: Instant) -> Enter<'_> {
    let mut entered = Enter::Wait;
    gate.send_if_modified(|state| match state {
        Gate::Open => {
            entered = Enter::Pass(Pass { gate, probe: false });
            false
        }
        Gate::Paused(paused) if now < paused.until => {
            entered = Enter::Paused(paused.error.clone());
            false
        }
        Gate::Untried | Gate::Paused(_) => {
            let paused = match state {
                Gate::Paused(paused) => Some(paused.clone()),
                _ => None,
            };
            *state = Gate::Probing { paused };
            entered = Enter::Pass(Pass { gate, probe: true });
            true
        }
        Gate::Probing { .. } => false,
    });
    entered
}

/// Leave to read a host. A probe dropped before it settles, by a timeout or a cancelled read,
/// gives its turn to the next waiting read.
struct Pass<'g> {
    gate: &'g watch::Sender<Gate>,
    /// Whether other reads wait on this one until it settles.
    probe: bool,
}

impl Pass<'_> {
    fn settle(mut self, reach: Reach, now: Instant) {
        self.update(reach, now);
        self.probe = false;
    }

    fn update(&self, reach: Reach, now: Instant) {
        self.gate.send_modify(|state| {
            let next = match (reach, &*state) {
                (Reach::Answered, _) => Gate::Open,
                (Reach::None(error), Gate::Probing { paused }) => {
                    let failures = paused.as_ref().map_or(0, |paused| paused.failures);
                    Gate::Paused(Paused::after(failures.saturating_add(1), error, now))
                }
                // Another read already paused it.
                (Reach::None(_), Gate::Paused(paused)) => Gate::Paused(paused.clone()),
                (Reach::None(error), Gate::Open | Gate::Untried) => {
                    Gate::Paused(Paused::after(1, error, now))
                }
                (Reach::Unknown, Gate::Probing { paused }) => {
                    paused.clone().map_or(Gate::Untried, Gate::Paused)
                }
                (Reach::Unknown, other) => other.clone(),
            };
            *state = next;
        });
    }
}

impl Drop for Pass<'_> {
    fn drop(&mut self) {
        if self.probe {
            self.update(Reach::Unknown, Instant::now());
        }
    }
}

/// What one read says about whether the host can be reached.
enum Reach {
    /// The host or its CLI answered, even with an error.
    Answered,
    /// The host could not be reached, with the error to give meanwhile.
    None(String),
    /// The machine's shell failed, which says nothing about the host.
    Unknown,
}

impl Reach {
    fn of(read: &Result<String>) -> Self {
        match read {
            Ok(_) => Reach::Answered,
            Err(error) => Self::of_error(error),
        }
    }

    fn of_error(error: &anyhow::Error) -> Self {
        if error.downcast_ref::<HostTimeout>().is_some() {
            return Reach::None(format!("{error:#}"));
        }

        match error.downcast_ref::<HostExit>() {
            Some(exit) if no_connection(&exit.tail) => Reach::None(format!("{error:#}")),
            Some(_) => Reach::Answered,
            None => Reach::Unknown,
        }
    }
}

/// Whether a CLI's `output` says it could not connect to the host.
fn no_connection(output: &str) -> bool {
    let output = output.to_lowercase();
    NO_CONNECTION.iter().any(|text| output.contains(text))
}

#[cfg(test)]
#[path = "../tests/issues_gate.rs"]
mod tests;
