//! One action worker per machine, independent of topic subscriptions.

use crate::{
    error::{DesktopError, Result},
    fleet::Fleet,
};
use beep_mesh::RETRY;
use beep_proto::node::{
    commands::{Reply, Request},
    events::{CallReply, Operation},
};
use iroh::EndpointId;
use std::{
    collections::HashMap,
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};
use tokio::{
    sync::{mpsc, oneshot},
    time::{Sleep, sleep, timeout},
};
use uuid::Uuid;

/// One order for a machine, and where its answer goes.
pub(super) struct Call {
    pub(super) operation: Operation,
    pub(super) reply: oneshot::Sender<std::result::Result<CallReply, String>>,
}
pub(super) type Commands = Arc<Mutex<HashMap<EndpointId, (Uuid, mpsc::Sender<Call>)>>>;

const IDLE: Duration = Duration::from_secs(35);
const SEND_LIMIT: Duration = Duration::from_secs(15);

/// A deadline across `select!` iterations so inbound calls do not reset the dead-link timer.
struct Idle(Pin<Box<Sleep>>);

impl Idle {
    fn new() -> Self {
        Self(Box::pin(sleep(IDLE)))
    }

    fn reset(&mut self) {
        self.0.as_mut().reset(tokio::time::Instant::now() + IDLE);
    }

    async fn expired(&mut self) {
        self.0.as_mut().await;
    }
}

pub(super) struct CommandWorker {
    pub(super) fleet: Arc<Fleet>,
    pub(super) machine: EndpointId,
    pub(super) commands: Commands,
}

/// Runs `connect` again `RETRY` after every end. `ended` logs an end with
/// the error and the connection's milliseconds.
pub(super) async fn retrying<F, Fut>(connect: F, ended: impl Fn(&DesktopError, u128))
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<()>>,
{
    loop {
        let started = Instant::now();
        if let Err(error) = connect().await {
            ended(&error, started.elapsed().as_millis());
        }

        sleep(RETRY).await;
    }
}

impl CommandWorker {
    pub(super) async fn run(self) {
        retrying(
            || self.connect(),
            |error, elapsed_ms| {
                log::warn!(
                    "command stream ended machine={} elapsed_ms={elapsed_ms}: {error}",
                    self.machine
                );
            },
        )
        .await;
    }

    async fn connect(&self) -> Result<()> {
        let client = self.fleet.client(self.machine).await?;
        let generation = Uuid::new_v4();
        let (mut tx, mut rx) = client.open_commands().await?;
        let (orders, mut input) = mpsc::channel::<Call>(16);
        let (wire_tx, mut wire_rx) = mpsc::channel::<Request>(16);
        let writer = tokio::spawn(async move {
            while let Some(request) = wire_rx.recv().await {
                timeout(SEND_LIMIT, tx.send(&request))
                    .await
                    .map_err(|_| DesktopError::Unsure("the machine".into()))??;
            }
            Ok::<(), DesktopError>(())
        });
        let _abort_writer = AbortWriter(writer.abort_handle());
        tokio::pin!(writer);
        let _registered = Registered {
            machine: self.machine,
            commands: &self.commands,
            generation,
        };
        self.commands
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(self.machine, (generation, orders));
        let mut pending = HashMap::new();
        let mut next = 1_u64;
        let mut idle = Idle::new();

        loop {
            tokio::select! {
                call = input.recv(), if pending.len() < 16 => if let Some(call) = call {
                    next = next.checked_add(1).ok_or_else(|| DesktopError::BadRequest("command sequence exhausted".into()))?;
                    wire_tx
                        .send(Request::Call { id: next, operation: Box::new(call.operation) })
                        .await
                        .map_err(|_| DesktopError::Lost("the machine".into()))?;
                    pending.insert(next, call.reply);
                },
                wrote = &mut writer => {
                    return match wrote {
                        Ok(Err(error)) => Err(error),
                        Ok(Ok(())) | Err(_) => Err(DesktopError::Lost("the machine".into())),
                    };
                },
                () = idle.expired() => {
                    return Err(DesktopError::Lost("the machine".into()));
                },
                reply = rx.recv::<Reply>() => {
                    let reply = reply?
                        .ok_or_else(|| DesktopError::Lost("the machine".into()))?;
                    idle.reset();

                    match reply {
                        Reply::Result { id, reply } => if let Some(waiter) = pending.remove(&id) {
                            let _ = waiter.send(Ok(*reply));
                        },
                        Reply::Error { id, reason } => if let Some(waiter) = pending.remove(&id) {
                            let _ = waiter.send(Err(reason));
                        },
                        _ => {},
                    }
                }
            }
        }
    }
}

struct AbortWriter(tokio::task::AbortHandle);

impl Drop for AbortWriter {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub(super) struct Registered<'a> {
    pub(super) machine: EndpointId,
    pub(super) commands: &'a Commands,
    pub(super) generation: Uuid,
}

impl Drop for Registered<'_> {
    fn drop(&mut self) {
        let mut commands = self.commands.lock().unwrap_or_else(PoisonError::into_inner);

        if commands
            .get(&self.machine)
            .is_some_and(|(id, _)| *id == self.generation)
        {
            commands.remove(&self.machine);
        }
    }
}
