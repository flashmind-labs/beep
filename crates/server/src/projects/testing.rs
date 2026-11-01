//! A fake fleet for the project tests: machines with recording shells and
//! shared files kept in memory.

use super::{
    Projects,
    commands::{CommandOutput, Shell},
    fleet::{Fleet, Machine},
    model::WorkerAgent,
};
use crate::protocol::node::{
    Capability,
    files::{FileEntry, FileKind, Reply, Request},
};
use anyhow::{Result, bail};
use async_trait::async_trait;
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use iroh::{EndpointId, SecretKey};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::Notify;

/// A shell that records each command and answers the first rule whose text
/// the command holds, else exits 0 printing nothing.
#[derive(Default)]
pub(crate) struct FakeShell {
    ran: Mutex<Vec<String>>,
    rules: Mutex<Vec<(String, CommandOutput)>>,
    /// Commands holding this text never finish.
    hangs: Mutex<Option<String>>,
    /// Commands holding the text wait for its signal before they answer.
    blocks: Mutex<Vec<(String, Arc<Notify>)>>,
}

impl FakeShell {
    pub(crate) fn hang_on(&self, holds: &str) {
        *self.hangs.lock().unwrap_or_else(PoisonError::into_inner) = Some(holds.to_owned());
    }

    /// Makes commands holding `holds` wait until the returned signal fires.
    pub(crate) fn block_on(&self, holds: &str) -> Arc<Notify> {
        let signal = Arc::new(Notify::new());
        self.blocks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((holds.to_owned(), signal.clone()));
        signal
    }

    pub(crate) fn answer(&self, holds: &str, code: i32, out: &str) {
        self.rules
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(
                0,
                (
                    holds.to_owned(),
                    CommandOutput {
                        code: Some(code),
                        out: out.to_owned(),
                    },
                ),
            );
    }

    pub(crate) fn ran(&self) -> Vec<String> {
        self.ran
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The commands that held `text`.
    pub(crate) fn ran_with(&self, text: &str) -> Vec<String> {
        self.ran()
            .into_iter()
            .filter(|command| command.contains(text))
            .collect()
    }
}

#[async_trait]
impl Shell for FakeShell {
    async fn run(&self, command: &str, _limit: Duration) -> Result<CommandOutput> {
        self.ran
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(command.to_owned());
        let hangs = self
            .hangs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if hangs.is_some_and(|holds| command.contains(&holds)) {
            std::future::pending::<()>().await;
        }
        let block = self
            .blocks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .find(|(holds, _)| command.contains(holds.as_str()))
            .map(|(_, signal)| signal.clone());
        if let Some(signal) = block {
            signal.notified().await;
        }
        let rules = self.rules.lock().unwrap_or_else(PoisonError::into_inner);
        Ok(rules
            .iter()
            .find(|(holds, _)| command.contains(holds.as_str()))
            .map_or(
                CommandOutput {
                    code: Some(0),
                    out: String::new(),
                },
                |(_, output)| output.clone(),
            ))
    }
}

/// A tool that counts its calls and answers "ran".
pub(crate) struct RecordingTool {
    pub(crate) name: &'static str,
    pub(crate) calls: AtomicUsize,
}

impl RecordingTool {
    pub(crate) fn new(name: &'static str) -> Self {
        Self {
            name,
            calls: AtomicUsize::new(0),
        }
    }

    pub(crate) fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl Tool for RecordingTool {
    fn name(&self) -> &str {
        self.name
    }

    fn description(&self) -> &str {
        "records its calls"
    }

    fn parameters(&self) -> Value {
        json!({"type": "object"})
    }

    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(ToolResult::success(ctx.tool_call_id, "ran"))
    }
}

pub(crate) fn machine_id(seed: u8) -> EndpointId {
    SecretKey::from_bytes(&[seed; 32]).public()
}

#[derive(Default)]
pub(crate) struct FakeFleet {
    pub(crate) machines: Vec<Machine>,
    pub(crate) shells: HashMap<EndpointId, Arc<FakeShell>>,
    pub(crate) roots: HashMap<EndpointId, Vec<String>>,
    /// Files by path, on any machine.
    pub(crate) files: HashMap<String, String>,
    pub(crate) projects: Projects,
    /// The sessions closed, by machine.
    pub(crate) closed: Mutex<Vec<(EndpointId, String)>>,
}

impl FakeFleet {
    /// Adds an online machine with a shell, sharing `root`.
    pub(crate) fn add(
        &mut self,
        seed: u8,
        name: &str,
        capabilities: &[Capability],
        root: &str,
    ) -> (EndpointId, Arc<FakeShell>) {
        let id = machine_id(seed);
        self.machines.push(Machine {
            id,
            name: name.to_owned(),
            online: true,
            capabilities: capabilities.to_vec(),
            home: Some("/home/sam".to_owned()),
            hostname: None,
        });
        let shell = Arc::new(FakeShell::default());
        self.shells.insert(id, shell.clone());
        self.roots.insert(id, vec![root.to_owned()]);
        (id, shell)
    }
}

#[async_trait]
impl Fleet for FakeFleet {
    async fn machines(&self) -> Result<Vec<Machine>> {
        Ok(self.machines.clone())
    }

    fn shell(&self, machine: &EndpointId) -> Option<Arc<dyn Shell>> {
        self.shells
            .get(machine)
            .map(|shell| shell.clone() as Arc<dyn Shell>)
    }

    async fn files(&self, machine: &EndpointId, request: &Request) -> Result<Reply> {
        Ok(match request {
            Request::Roots {} => Reply::Roots {
                roots: self.roots.get(machine).cloned().unwrap_or_default(),
            },
            Request::List { path, .. } => {
                let prefix = format!("{path}/");
                let entries = self
                    .files
                    .iter()
                    .filter_map(|(file, data)| {
                        let name = file.strip_prefix(&prefix)?;
                        (!name.contains('/')).then(|| FileEntry {
                            name: name.to_owned(),
                            kind: FileKind::File,
                            size: data.len() as u64,
                            modified: None,
                        })
                    })
                    .collect();
                Reply::Entries {
                    path: path.clone(),
                    entries,
                    more: 0,
                }
            }
            Request::Read { path, .. } => match self.files.get(path) {
                Some(data) => Reply::Data {
                    path: path.clone(),
                    data: data.clone().into_bytes(),
                    size: data.len() as u64,
                },
                None => Reply::Error {
                    message: format!("no file {path}"),
                },
            },
            _ => bail!("the fake fleet does not do that"),
        })
    }

    fn projects(&self) -> &Projects {
        &self.projects
    }

    async fn close_session(
        &self,
        machine: &EndpointId,
        _agent: WorkerAgent,
        session: &str,
    ) -> Result<()> {
        self.closed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((*machine, session.to_owned()));
        Ok(())
    }
}
