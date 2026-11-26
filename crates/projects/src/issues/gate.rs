//! Remembering issue hosts a machine could not reach, so their reads stop for a while instead of
//! each starting the host's CLI again.

use super::command::{HostExit, HostTimeout, IssueSource, run_rendered};
use crate::{Shell, model::IssueHost};
use anyhow::{Result, anyhow};
use beep_proto::watched;
use futures::{
    FutureExt,
    future::{BoxFuture, Shared, WeakShared},
};
use iroh::EndpointId;
use std::{
    collections::HashMap,
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

/// A read that did not reach the host, now or when it was last tried. The gate logs it when the
/// host pauses, so the reads that get it do not.
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
/// row, and while it is unknown one read asks it and the others wait for that one. A read joins
/// an identical one under way on the same machine, and the machine that last answered for a host
/// is asked first.
#[derive(Default)]
pub struct HostGates {
    known: Mutex<Known>,
}

#[derive(Default)]
struct Known {
    gates: HashMap<HostOnMachine, Arc<watch::Sender<Gate>>>,
    /// The machine that last answered for each host.
    answered: HashMap<IssueHost, EndpointId>,
    /// The reads under way, by machine and command line.
    reads: HashMap<(EndpointId, String), WeakShared<SharedRead>>,
}

/// One read that every identical read at the same time waits on.
type SharedRead = BoxFuture<'static, Result<String, Arc<anyhow::Error>>>;

/// A host as one machine reaches it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct HostOnMachine {
    machine: EndpointId,
    host: IssueHost,
}

impl HostGates {
    /// Runs `rendered` on `source`'s machine unless its host is paused there, which gives the
    /// remembered [`Unreachable`] at once. With `join`, for a command that only reads, a read
    /// identical to one under way waits for that one's answer instead of running the CLI again.
    pub async fn run(&self, source: &IssueSource, rendered: &str, join: bool) -> Result<String> {
        let key = HostOnMachine {
            machine: source.machine,
            host: source.host.clone(),
        };
        let read = if join {
            self.joined(&key, source.shell.clone(), rendered)
        } else {
            gated(
                self.gate(&key),
                key.clone(),
                source.shell.clone(),
                rendered.to_owned(),
            )
            .boxed()
            .shared()
        };
        let result = read.await.map_err(|error| rebuilt(&error));

        if result.is_ok() {
            self.lock().answered.insert(key.host, key.machine);
        }

        result
    }

    /// The machine that last answered for `host`.
    pub fn preferred(&self, host: &IssueHost) -> Option<EndpointId> {
        self.lock().answered.get(host).copied()
    }

    /// Whether `host` is paused on `machine` now, so a read there would not ask it.
    pub fn is_paused(&self, machine: EndpointId, host: &IssueHost) -> bool {
        let key = HostOnMachine {
            machine,
            host: host.clone(),
        };
        let known = self.lock();
        known.gates.get(&key).is_some_and(|gate| {
            watched::read_sent(
                gate,
                |gate| matches!(gate, Gate::Paused(paused) if Instant::now() < paused.until),
            )
        })
    }

    /// The read of `rendered` on the key's machine under way, else a new one.
    fn joined(
        &self,
        key: &HostOnMachine,
        shell: Arc<dyn Shell>,
        rendered: &str,
    ) -> Shared<SharedRead> {
        let gate = self.gate(key);
        let mut known = self.lock();
        known.reads.retain(|_, read| read.upgrade().is_some());
        let id = (key.machine, rendered.to_owned());

        if let Some(read) = known.reads.get(&id).and_then(WeakShared::upgrade) {
            return read;
        }

        let read = gated(gate, key.clone(), shell, rendered.to_owned())
            .boxed()
            .shared();
        if let Some(weak) = read.downgrade() {
            known.reads.insert(id, weak);
        }

        read
    }

    fn gate(&self, key: &HostOnMachine) -> Arc<watch::Sender<Gate>> {
        self.lock()
            .gates
            .entry(key.clone())
            .or_insert_with(|| Arc::new(watch::Sender::new(Gate::Untried)))
            .clone()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Known> {
        self.known.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Runs `rendered` through `gate`: at once when the host answers, after the probe under way
/// when it is unknown, and not at all while it is paused.
async fn gated(
    gate: Arc<watch::Sender<Gate>>,
    key: HostOnMachine,
    shell: Arc<dyn Shell>,
    rendered: String,
) -> Result<String, Arc<anyhow::Error>> {
    loop {
        let pass = match enter(&gate, &key, Instant::now()) {
            Enter::Pass(pass) => pass,
            Enter::Paused(error) => return Err(Arc::new(Unreachable(error).into())),
            Enter::Wait => {
                let mut changed = gate.subscribe();
                // The sender is held here, so the channel stays open.
                let _ = watched::wait_for(&mut changed, |gate| !gate.is_probing()).await;
                continue;
            }
        };
        let result = run_rendered(shell.as_ref(), &rendered).await;
        let reach = Reach::of(&result);
        let result = match &reach {
            Reach::None(error) => Err(Unreachable(error.clone()).into()),
            _ => result,
        };

        pass.settle(reach, Instant::now());
        return result.map_err(Arc::new);
    }
}

/// A copy of a shared read's `error`, keeping the kinds the issue code tells apart.
fn rebuilt(error: &anyhow::Error) -> anyhow::Error {
    if let Some(unreachable) = error.downcast_ref::<Unreachable>() {
        return unreachable.clone().into();
    }
    if let Some(exit) = error.downcast_ref::<HostExit>() {
        return exit.clone().into();
    }
    if error.downcast_ref::<HostTimeout>().is_some() {
        return HostTimeout.into();
    }

    anyhow!("{error:#}")
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

fn enter<'g>(gate: &'g watch::Sender<Gate>, key: &'g HostOnMachine, now: Instant) -> Enter<'g> {
    let mut entered = Enter::Wait;
    gate.send_if_modified(|state| match state {
        Gate::Open => {
            entered = Enter::Pass(Pass {
                gate,
                key,
                probe: false,
            });
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
            entered = Enter::Pass(Pass {
                gate,
                key,
                probe: true,
            });
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
    key: &'g HostOnMachine,
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
            self.key.log_change(state, &next);
            *state = next;
        });
    }
}

impl HostOnMachine {
    /// Logs the gate going from `before` to `after` when the host paused or answers again, once
    /// per change rather than once per read.
    fn log_change(&self, before: &Gate, after: &Gate) {
        let (machine, host) = (self.machine.fmt_short(), &self.host);

        match (before, after) {
            (Gate::Probing { paused: Some(_) }, Gate::Paused(paused)) => log::info!(
                "projects: {machine} still cannot reach {host}, tried again in {} s",
                pause(paused.failures).as_secs()
            ),
            (Gate::Probing { paused: None } | Gate::Open | Gate::Untried, Gate::Paused(paused)) => {
                log::info!(
                    "projects: {machine} cannot reach {host}, tried again in {} s: {}",
                    pause(paused.failures).as_secs(),
                    paused.error
                )
            }
            (Gate::Probing { paused: Some(_) } | Gate::Paused(_), Gate::Open) => {
                log::info!("projects: {machine} reaches {host} again")
            }
            _ => {}
        }
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
