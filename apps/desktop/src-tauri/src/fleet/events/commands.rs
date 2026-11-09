//! One action worker per machine, independent of topic subscriptions.

use super::{Call, Commands, Fleet};
use crate::error::{DesktopError, Result};
use beep_proto::node::commands::{Reply, Request};
use iroh::EndpointId;
use std::{
    collections::HashMap,
    sync::{Arc, PoisonError},
    time::{Duration, Instant},
};
use tokio::{
    sync::mpsc,
    time::{sleep, timeout},
};
use uuid::Uuid;

pub(super) struct CommandWorker {
    fleet: Arc<Fleet>,
    machine: EndpointId,
    commands: Commands,
}

impl CommandWorker {
    pub(super) fn new(fleet: Arc<Fleet>, machine: EndpointId, commands: Commands) -> Self {
        Self {
            fleet,
            machine,
            commands,
        }
    }

    pub(super) async fn run(self) {
        let mut pause = Duration::from_secs(1);

        loop {
            let started = Instant::now();
            if let Err(error) = self.connect().await {
                log::warn!(
                    "command stream ended machine={} elapsed_ms={}: {error}",
                    self.machine,
                    started.elapsed().as_millis()
                );
            }

            if started.elapsed() >= Duration::from_secs(10) {
                pause = Duration::from_secs(1);
            }

            sleep(pause).await;
            pause = (pause * 2).min(Duration::from_secs(30));
        }
    }

    async fn connect(&self) -> Result<()> {
        let client = self.fleet.client(self.machine).await?;
        let generation = Uuid::new_v4();
        let (mut tx, mut rx) = client.open_commands().await?;
        let (orders, mut input) = mpsc::channel::<Call>(16);
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

        loop {
            tokio::select! {
                call = input.recv(), if pending.len() < 16 => if let Some(call) = call {
                    next = next.checked_add(1).ok_or_else(|| DesktopError::BadRequest("command sequence exhausted".into()))?;
                    timeout(Duration::from_secs(15), tx.send( &Request::Call { id: next, operation: Box::new(call.operation) }))
                        .await.map_err(|_| DesktopError::Unsure("the machine".into()))??;
                    pending.insert(next, call.reply);
                },
                reply = timeout(Duration::from_secs(35), rx.recv::<Reply>()) => {
                    let reply = reply.map_err(|_| DesktopError::Lost("the machine".into()))??
                        .ok_or_else(|| DesktopError::Lost("the machine".into()))?;

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
