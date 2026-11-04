//! A fake fleet for project tests: recording shells and in-memory shared files.

use super::{
    Projects,
    fleet::{Fleet, Machine, names_of},
    model::{StartRequest, WorkerAgent},
    worker::{StartError, Started},
};
use crate::{
    nodes::SessionRefused,
    protocol::node::{
        Capability,
        agent::{Question, Request as AgentRequest, SessionInfo, State as AgentState, Status},
        files::{FileEntry, FileKind, Reply, Request},
    },
    store::{ProjectRow, TaskRow, WorkerRow},
};
use anyhow::{Result, bail};
use async_trait::async_trait;
use beep_projects::{
    Shell, ShellOutput,
    issues::{MachineName, Machines},
    model::TaskSlug,
};
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use iroh::{EndpointId, SecretKey};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::Notify;
use uuid::Uuid;

/// A shell that records commands and answers by the first matching rule, else exits 0 silently.
#[derive(Default)]
pub(crate) struct FakeShell {
    ran: Mutex<Vec<String>>,
    rules: Mutex<Vec<(String, ShellOutput)>>,
    /// Commands holding this text fail as if the machine dropped.
    fails: Mutex<Option<String>>,
    /// Commands holding the text wait for its signal before they answer.
    blocks: Mutex<Vec<(String, Arc<Notify>)>>,
    /// Signalled when a command starts waiting for its signal.
    blocked: Notify,
}

impl FakeShell {
    pub(crate) fn fail_on(&self, holds: &str) {
        *self.fails.lock().unwrap_or_else(PoisonError::into_inner) = Some(holds.to_owned());
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

    /// Waits until a command waits for its signal.
    pub(crate) async fn until_blocked(&self) {
        self.blocked.notified().await;
    }

    pub(crate) fn answer(&self, holds: &str, code: i32, out: &str) {
        self.rules
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(0, (holds.to_owned(), ShellOutput::exited(code, out)));
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
    async fn run(&self, command: &str, _limit: Duration) -> Result<ShellOutput> {
        self.ran
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(command.to_owned());
        let fails = self
            .fails
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if fails.is_some_and(|holds| command.contains(&holds)) {
            bail!("the machine dropped");
        }
        let block = self
            .blocks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .find(|(holds, _)| command.contains(holds.as_str()))
            .map(|(_, signal)| signal.clone());
        if let Some(signal) = block {
            self.blocked.notify_one();
            signal.notified().await;
        }
        let rules = self.rules.lock().unwrap_or_else(PoisonError::into_inner);
        Ok(rules
            .iter()
            .find(|(holds, _)| command.contains(holds.as_str()))
            .map_or(ShellOutput::exited(0, ""), |(_, output)| output.clone()))
    }
}

/// A shell that runs `prefix` first, so a test can put fakes on `PATH` without touching env.
pub(crate) struct Prefixed {
    pub(crate) prefix: String,
    pub(crate) shell: Arc<dyn Shell>,
}

#[async_trait]
impl Shell for Prefixed {
    async fn run(&self, command: &str, limit: Duration) -> Result<ShellOutput> {
        self.shell
            .run(&format!("{}\n{command}", self.prefix), limit)
            .await
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

/// Alice's `acme/widget`, set up with a home checkout on `home`.
pub(crate) async fn widget_project(
    user: &crate::account::user::User,
    home: EndpointId,
    roles: beep_projects::model::Roles,
    visibility: beep_projects::model::Visibility,
) -> crate::store::ProjectRow {
    let project = user
        .ensure_project(&beep_projects::model::RepoName::parse("acme/widget").unwrap())
        .await
        .unwrap();
    user.add_checkout(
        project.id,
        &crate::store::CheckoutRow {
            machine: home,
            path: beep_projects::model::MachinePath::parse("/srv/widget").unwrap(),
            home: true,
        },
    )
    .await
    .unwrap();
    user.set_project_setup(project.id, &roles, visibility, "main")
        .await
        .unwrap();
    user.project(project.id).await.unwrap().unwrap()
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
    /// The turns posted, by label.
    pub(crate) posted: Mutex<Vec<(String, String)>>,
    /// The tasks started, which all queue.
    pub(crate) started: Mutex<Vec<TaskSlug>>,
    /// When set, every start fails saying this.
    pub(crate) start_fails: Mutex<Option<String>>,
    /// What went to sessions.
    pub(crate) sent: Mutex<Vec<AgentRequest>>,
    /// When set, sends to sessions fail, as on a machine that dropped.
    pub(crate) send_fails: AtomicBool,
    /// The questions put to the user.
    pub(crate) asked: Mutex<Vec<Question>>,
    /// The status [`Fleet::session_info`] gives; none fails it.
    pub(crate) live: Mutex<Option<Status>>,
    /// What [`Fleet::attach`] answers.
    pub(crate) attach: Mutex<FakeAttach>,
}

/// What the fake fleet's `attach` answers.
#[derive(Default)]
pub(crate) enum FakeAttach {
    /// The stream fails, as on a machine that flaps.
    #[default]
    Fails,
    /// The machine no longer has the session.
    Refused,
}

fn take<T: Clone>(list: &Mutex<Vec<T>>) -> Vec<T> {
    list.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

fn push<T>(list: &Mutex<Vec<T>>, item: T) {
    list.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(item);
}

impl FakeFleet {
    pub(crate) fn posts(&self) -> Vec<(String, String)> {
        take(&self.posted)
    }

    pub(crate) fn starts(&self) -> Vec<TaskSlug> {
        take(&self.started)
    }

    pub(crate) fn sends(&self) -> Vec<AgentRequest> {
        take(&self.sent)
    }

    pub(crate) fn questions(&self) -> Vec<Question> {
        take(&self.asked)
    }

    pub(crate) fn closes(&self) -> Vec<(EndpointId, String)> {
        take(&self.closed)
    }
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
            syncthing: Vec::new(),
        });
        let shell = Arc::new(FakeShell::default());
        self.shells.insert(id, shell.clone());
        self.roots.insert(id, vec![root.to_owned()]);
        (id, shell)
    }
}

#[async_trait]
impl Machines for FakeFleet {
    fn shell(&self, machine: &EndpointId) -> Option<Arc<dyn Shell>> {
        self.shells
            .get(machine)
            .map(|shell| shell.clone() as Arc<dyn Shell>)
    }

    async fn names(&self) -> Vec<MachineName> {
        names_of(self).await
    }
}

#[async_trait]
impl Fleet for FakeFleet {
    async fn machines(&self) -> Result<Vec<Machine>> {
        Ok(self.machines.clone())
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
        push(&self.closed, (*machine, session.to_owned()));
        Ok(())
    }

    fn post(&self, _chat: Uuid, label: &str, text: String) {
        push(&self.posted, (label.to_owned(), text));
    }

    async fn start_task(
        &self,
        _project: &ProjectRow,
        task: &TaskRow,
        _request: &StartRequest,
    ) -> Result<Started, StartError> {
        push(&self.started, task.slug.clone());
        let fails = self
            .start_fails
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        match fails {
            Some(why) => Err(StartError::Node(why)),
            None => Ok(Started::Queued("the fake fleet starts nothing".to_owned())),
        }
    }

    async fn send_session(&self, _worker: &WorkerRow, request: AgentRequest) -> Result<()> {
        if self.send_fails.load(Ordering::Relaxed) {
            bail!("the machine dropped");
        }
        push(&self.sent, request);
        Ok(())
    }

    async fn ask_user(
        &self,
        _worker: &WorkerRow,
        _task: &TaskRow,
        _chat: Uuid,
        question: Question,
    ) -> Result<()> {
        push(&self.asked, question);
        Ok(())
    }

    async fn session_info(&self, worker: &WorkerRow) -> Result<SessionInfo> {
        let Some(status) = *self.live.lock().unwrap_or_else(PoisonError::into_inner) else {
            bail!("the machine is offline");
        };
        Ok(SessionInfo {
            id: worker.session.clone().unwrap_or_default(),
            state: AgentState {
                status,
                ..AgentState::default()
            },
            ..SessionInfo::default()
        })
    }

    async fn attach(&self, worker: &WorkerRow, _chat: Uuid) -> Result<SessionInfo> {
        match &*self.attach.lock().unwrap_or_else(PoisonError::into_inner) {
            FakeAttach::Fails => bail!("the node closed the session stream"),
            FakeAttach::Refused => Err(SessionRefused {
                reason: format!("there is no session {:?}", worker.session),
            }
            .into()),
        }
    }
}
