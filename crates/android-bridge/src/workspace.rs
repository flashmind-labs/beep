//! Mobile access to existing workspace requests. No wire types live here.

use crate::{AsyncMutex, BeepClient, BeepError, CALL_TIMEOUT, timed};
use beep_fleet::ImportJob;
use beep_mesh::machine::{
    agent_files,
    terminals::{self, Heard},
};
use beep_proto::{
    AgentCheckoutPlan, AgentEffort, AgentHost, AgentModels, AgentSessionView, AgentSteer,
    AnswerAgentQuestion, CloseTerminal, GetAgentSession, GetBranchDiff, GetProjectAgent,
    GetProjectLists, GetProjectRun, GetPullRequestHead, GetTerminal, GetThreadDetail, GetWorkDiff,
    GitHubAccount, ImportProject, ImportedProject, ListAgentHosts, ListAgentModels,
    ListAgentSessions, ListAllTerminals, ListGitHubAccounts, ListImportedProjects,
    ListProjectBranches, ListProjectMachines, ListProjectRuns, ListProjectSessions, ListProjects,
    MachineSession, ProjectAgent, ProjectBranches, ProjectIssue, ProjectMachine,
    ProjectPullRequest, PullRequestHead, ResumeAgentSession, RunState, StartAgentSession,
    StartedAgent, SteerAgentSession, Terminal, TerminalOutput, ThreadDetail, WorkDiff, WorkflowRun,
};
use beep_proto::{
    AgentSessionUsage, GetAgentSessionUsage, GetInstructions, GetUsageReport, InstructionsVersion,
    ListInstructionsVersions, MachineAgentFiles, ReadAgentFiles, SaveInstructions,
};
use beep_proto::{
    AttachTerminal, ChatApproval, GetProjectSyncDetail, ListPendingApprovals, MAX_TERMINAL_INPUT,
    OpenTerminal, ProjectSyncDetail, TerminalEvent, TerminalInput,
    client::{Replies, Uplink},
};
use beep_proto::{
    ListProjectsOnTop, PauseProjectSync, PinProjectToTop, ProjectRunDetail, RemoveProject,
    node::{
        LinkRx, LinkTx,
        agent::Request as AgentRequest,
        terminal::{Reply as TerminalReply, Request as TerminalRequest},
    },
};
use std::{sync::Arc, time::SystemTime};
use uuid::Uuid;

#[derive(uniffi::Record)]
pub struct CodingStart {
    pub machine_id: String,
    pub agent: String,
    pub cwd: Option<String>,
    pub prompt: String,
    pub model: Option<String>,
    pub effort: Option<AgentEffort>,
    pub checkout: Option<AgentCheckoutPlan>,
}

#[derive(uniffi::Record)]
pub struct CommandStart {
    pub project: String,
    pub machine_id: String,
    pub name: String,
    pub command: String,
    pub cwd: String,
}

// These view records expose replies whose wire structs are not FFI records.
#[derive(uniffi::Record)]
pub struct MobileProjectLists {
    pub pull_requests: Vec<ProjectPullRequest>,
    pub issues: Vec<ProjectIssue>,
    pub prs_error: Option<String>,
    pub issues_error: Option<String>,
}

#[derive(uniffi::Record)]
pub struct MobileRun {
    pub id: u64,
    pub number: u64,
    pub workflow: String,
    pub title: String,
    pub branch: String,
    pub event: String,
    pub state: RunState,
    pub url: String,
    pub started_at: Option<SystemTime>,
    pub finished_at: Option<SystemTime>,
}

impl From<WorkflowRun> for MobileRun {
    fn from(run: WorkflowRun) -> Self {
        Self {
            id: run.id,
            number: run.number,
            workflow: run.workflow,
            title: run.title,
            branch: run.branch,
            event: run.event,
            state: run.state,
            url: run.url,
            started_at: run.started_at,
            finished_at: run.finished_at,
        }
    }
}

#[derive(uniffi::Record)]
pub struct MobileRuns {
    pub runs: Vec<MobileRun>,
    pub error: Option<String>,
}

#[derive(uniffi::Record)]
pub struct MobileJob {
    pub name: String,
    pub state: RunState,
    pub steps: Vec<MobileStep>,
}

#[derive(uniffi::Record)]
pub struct MobileStep {
    pub name: String,
    pub state: RunState,
}

#[derive(uniffi::Record)]
pub struct MobileRunDetail {
    pub run: MobileRun,
    pub jobs: Vec<MobileJob>,
    pub log: Option<String>,
    pub error: Option<String>,
}

#[derive(uniffi::Record)]
pub struct MobileUsage {
    pub cost_usd: f64,
    pub tokens: u64,
    pub limits: Vec<MobileLimit>,
}

#[derive(uniffi::Record)]
pub struct MobileLimit {
    pub agent: String,
    pub minutes: u32,
    pub used_percent: f64,
}

#[uniffi::export(async_runtime = "tokio")]
impl BeepClient {
    pub async fn instructions(&self) -> Result<Option<InstructionsVersion>, BeepError> {
        if let Some(mesh) = self.mesh().await? {
            return Ok(mesh.desk().await?.instructions());
        }
        self.call(GetInstructions {}).await
    }

    pub async fn instructions_history(&self) -> Result<Vec<InstructionsVersion>, BeepError> {
        if let Some(mesh) = self.mesh().await? {
            return Ok(mesh.desk().await?.instructions_versions());
        }
        self.call(ListInstructionsVersions {}).await
    }

    pub async fn save_instructions(
        &self,
        text: String,
        base: Option<u32>,
    ) -> Result<InstructionsVersion, BeepError> {
        if let Some(mesh) = self.mesh().await? {
            return Ok(mesh
                .desk()
                .await?
                .save_instructions(&text, base, "")
                .await?);
        }
        self.call(SaveInstructions {
            text,
            base,
            note: String::new(),
        })
        .await
    }

    pub async fn read_agent_files(&self) -> Result<Vec<MachineAgentFiles>, BeepError> {
        if let Some(mesh) = self.mesh().await? {
            let newest = mesh.desk().await?.instructions().map(|value| value.text);
            let mut files = Vec::new();
            for machine in mesh
                .machines()
                .await
                .into_iter()
                .filter(|machine| machine.online)
            {
                let client = mesh.client(&machine.id).await?;
                files.push(agent_files::machine_files(
                    machine.id,
                    machine.name,
                    agent_files::read(&client).await,
                    newest.as_deref(),
                ));
            }
            return Ok(files);
        }
        self.call(ReadAgentFiles {}).await
    }

    pub async fn coding_usage(&self) -> Result<MobileUsage, BeepError> {
        if let Some(mesh) = self.mesh().await? {
            let usage = mesh.desk().await?.usage_report().await?;
            return Ok(MobileUsage {
                cost_usd: usage.today.total.cost_usd,
                tokens: usage.today.tokens,
                limits: usage
                    .limits
                    .into_iter()
                    .flat_map(|limit| {
                        let agent = limit.agent.to_string();
                        limit.windows.into_iter().map(move |window| MobileLimit {
                            agent: agent.clone(),
                            minutes: window.minutes,
                            used_percent: window.used_percent,
                        })
                    })
                    .collect(),
            });
        }
        let usage = self.call(GetUsageReport { refresh: false }).await?;
        Ok(MobileUsage {
            cost_usd: usage.today.total.cost_usd,
            tokens: usage.today.tokens,
            limits: usage
                .limits
                .into_iter()
                .flat_map(|limit| {
                    let agent = limit.agent.to_string();
                    limit.windows.into_iter().map(move |window| MobileLimit {
                        agent: agent.clone(),
                        minutes: window.minutes,
                        used_percent: window.used_percent,
                    })
                })
                .collect(),
        })
    }

    pub async fn coding_session_usage(
        &self,
        machine_id: String,
        agent: String,
        session: String,
    ) -> Result<AgentSessionUsage, BeepError> {
        if let Some(mesh) = self.mesh().await? {
            return mesh.usage(&machine_id, &agent, &session).await;
        }
        self.call(GetAgentSessionUsage {
            machine_id,
            agent,
            session,
        })
        .await
    }

    pub async fn list_projects(&self) -> Result<Vec<ProjectAgent>, BeepError> {
        if let Some(mesh) = self.mesh().await? {
            return Ok(mesh.projects());
        }
        self.call(ListProjects {}).await
    }

    pub async fn list_imported_projects(&self) -> Result<Vec<ImportedProject>, BeepError> {
        if let Some(mesh) = self.mesh().await? {
            return Ok(mesh.desk().await?.imported().await);
        }
        self.call(ListImportedProjects {}).await
    }

    pub async fn project_agent(&self, project: String) -> Result<ProjectAgent, BeepError> {
        if let Some(mesh) = self.mesh().await? {
            return mesh
                .projects()
                .into_iter()
                .find(|known| known.name == project)
                .ok_or(BeepError::NotFound);
        }
        self.call(GetProjectAgent { project }).await
    }

    pub async fn import_project(
        &self,
        repo: String,
        machine_id: String,
        folder: Option<String>,
    ) -> Result<ImportedProject, BeepError> {
        if let Some(mesh) = self.mesh().await? {
            return Ok(mesh
                .desk()
                .await?
                .import(ImportJob {
                    repo: &repo,
                    machine_id: Some(&machine_id),
                    folder: folder.as_deref(),
                    parent: None,
                    worktrees: &[],
                })
                .await?);
        }
        self.call(ImportProject {
            repo,
            machine_id: Some(machine_id),
            folder,
            worktrees: Vec::new(),
            parent: None,
        })
        .await
    }

    pub async fn project_lists(
        &self,
        project_id: String,
        refresh: bool,
    ) -> Result<MobileProjectLists, BeepError> {
        if let Some(mesh) = self.mesh().await? {
            let project = mesh.project_name(&project_id).await?;
            let lists = mesh.desk().await?.lists(&project).await?.lists;
            return Ok(MobileProjectLists {
                pull_requests: lists.pull_requests,
                issues: lists.issues,
                prs_error: lists.prs_error,
                issues_error: lists.issues_error,
            });
        }
        let lists = self
            .call(GetProjectLists {
                project_id: parse_id(&project_id)?,
                refresh,
            })
            .await?;
        Ok(MobileProjectLists {
            pull_requests: lists.pull_requests,
            issues: lists.issues,
            prs_error: lists.prs_error,
            issues_error: lists.issues_error,
        })
    }

    pub async fn project_machines(
        &self,
        project: String,
    ) -> Result<Vec<ProjectMachine>, BeepError> {
        if let Some(mesh) = self.mesh().await? {
            return Ok(mesh.desk().await?.project_machines(&project).await?);
        }
        self.call(ListProjectMachines { project }).await
    }

    pub async fn workspace_approvals(&self) -> Result<Vec<ChatApproval>, BeepError> {
        if self.mesh().await?.is_some() {
            return Ok(Vec::new());
        }
        self.call(ListPendingApprovals {}).await
    }

    pub async fn project_sync(&self, project: String) -> Result<ProjectSyncDetail, BeepError> {
        if let Some(mesh) = self.mesh().await? {
            return Ok(mesh.desk().await?.sync_detail(&project).await?);
        }
        self.call(GetProjectSyncDetail { project }).await
    }

    pub async fn project_branches(&self, project: String) -> Result<ProjectBranches, BeepError> {
        if let Some(mesh) = self.mesh().await? {
            return Ok(mesh.desk().await?.branches(&project).await?);
        }
        self.call(ListProjectBranches { project }).await
    }

    pub async fn thread_detail(
        &self,
        project_id: String,
        number: u64,
    ) -> Result<ThreadDetail, BeepError> {
        if let Some(mesh) = self.mesh().await? {
            let project = mesh.project_name(&project_id).await?;
            return Ok(mesh.desk().await?.thread_detail(&project, number).await?);
        }
        self.call(GetThreadDetail {
            project_id: parse_id(&project_id)?,
            number,
        })
        .await
    }

    pub async fn pull_request_head(
        &self,
        project_id: String,
        number: u64,
    ) -> Result<PullRequestHead, BeepError> {
        if let Some(mesh) = self.mesh().await? {
            let project = mesh.project_name(&project_id).await?;
            return Ok(mesh.desk().await?.pr_head(&project, number).await?);
        }
        self.call(GetPullRequestHead {
            project_id: parse_id(&project_id)?,
            number,
        })
        .await
    }

    pub async fn branch_diff(
        &self,
        project: String,
        branch: String,
    ) -> Result<WorkDiff, BeepError> {
        if let Some(mesh) = self.mesh().await? {
            let place = mesh
                .desk()
                .await?
                .project_machines(&project)
                .await?
                .into_iter()
                .find(|place| place.online && place.folder.is_some())
                .ok_or_else(|| crate::bad_request("No checkout is online."))?;
            return mesh
                .diff(
                    &place.machine_id,
                    place.folder.as_deref().unwrap_or(""),
                    &branch,
                )
                .await;
        }
        self.call(GetBranchDiff {
            project,
            branch,
            only: Vec::new(),
            lockfiles: false,
        })
        .await
    }

    pub async fn session_diff(
        &self,
        machine_id: String,
        root: String,
    ) -> Result<WorkDiff, BeepError> {
        if let Some(mesh) = self.mesh().await? {
            return mesh.diff(&machine_id, &root, "").await;
        }
        self.call(GetWorkDiff {
            machine_id,
            root,
            base: String::new(),
            head: String::new(),
            only: Vec::new(),
            lockfiles: false,
        })
        .await
    }

    pub async fn project_runs(
        &self,
        project_id: String,
        refresh: bool,
    ) -> Result<MobileRuns, BeepError> {
        if let Some(mesh) = self.mesh().await? {
            let project = mesh.project_name(&project_id).await?;
            let found = mesh.desk().await?.runs(&project).await?;
            return Ok(MobileRuns {
                runs: found.runs.into_iter().map(|run| run.run.into()).collect(),
                error: found.error,
            });
        }
        let runs = self
            .call(ListProjectRuns {
                project_id: parse_id(&project_id)?,
                refresh,
            })
            .await?;
        Ok(MobileRuns {
            runs: runs.runs.into_iter().map(MobileRun::from).collect(),
            error: runs.error,
        })
    }

    pub async fn project_run(
        &self,
        project_id: String,
        run_id: u64,
    ) -> Result<MobileRunDetail, BeepError> {
        if let Some(mesh) = self.mesh().await? {
            let project = mesh.project_name(&project_id).await?;
            let detail = mesh.desk().await?.run(&project, None, run_id).await?;
            return Ok(MobileRunDetail::from(detail));
        }
        let detail = self
            .call(GetProjectRun {
                project_id: parse_id(&project_id)?,
                run_id,
            })
            .await?;
        Ok(MobileRunDetail::from(detail))
    }

    pub async fn github_accounts(&self) -> Result<Vec<GitHubAccount>, BeepError> {
        if let Some(mesh) = self.mesh().await? {
            return Ok(mesh.desk().await?.github_accounts());
        }
        self.call(ListGitHubAccounts {}).await
    }

    pub async fn coding_hosts(&self) -> Result<Vec<AgentHost>, BeepError> {
        if let Some(mesh) = self.mesh().await? {
            return Ok(mesh.hosts().await);
        }
        self.call(ListAgentHosts {}).await
    }

    pub async fn coding_sessions(
        &self,
        project: Option<String>,
    ) -> Result<Vec<MachineSession>, BeepError> {
        if let Some(mesh) = self.mesh().await? {
            return Ok(mesh.sessions(project.as_deref()).await);
        }
        match project {
            Some(project) => self.call(ListProjectSessions { project }).await,
            None => self.call(ListAgentSessions {}).await,
        }
    }

    pub async fn coding_session(
        &self,
        machine_id: String,
        agent: String,
        session: String,
    ) -> Result<AgentSessionView, BeepError> {
        if let Some(mesh) = self.mesh().await? {
            return mesh.session(&machine_id, &agent, &session).await;
        }
        self.call(GetAgentSession {
            machine_id,
            agent,
            session,
            last: 200,
            before: None,
        })
        .await
    }

    pub async fn coding_models(
        &self,
        machine_id: String,
        agent: String,
    ) -> Result<AgentModels, BeepError> {
        if let Some(mesh) = self.mesh().await? {
            return mesh.models(&machine_id, &agent).await;
        }
        self.call(ListAgentModels { machine_id, agent }).await
    }

    pub async fn start_coding(&self, start: CodingStart) -> Result<StartedAgent, BeepError> {
        if let Some(mesh) = self.mesh().await? {
            return mesh.start(start).await;
        }
        self.call(StartAgentSession {
            machine_id: Some(start.machine_id),
            agent: Some(start.agent),
            cwd: start.cwd,
            prompt: start.prompt,
            needs: Vec::new(),
            chat: None,
            model: start.model,
            effort: start.effort,
            checkout: start.checkout,
        })
        .await
    }

    pub async fn resume_coding(
        &self,
        machine_id: String,
        agent: String,
        session: String,
    ) -> Result<StartedAgent, BeepError> {
        if let Some(mesh) = self.mesh().await? {
            return mesh.resume(&machine_id, &agent, &session).await;
        }
        self.call(ResumeAgentSession {
            machine_id,
            agent,
            session,
            prompt: None,
        })
        .await
    }

    pub async fn steer_coding(
        &self,
        machine_id: String,
        agent: String,
        session: String,
        steer: AgentSteer,
    ) -> Result<(), BeepError> {
        if let Some(mesh) = self.mesh().await? {
            return mesh.steer(&machine_id, &agent, &session, steer).await;
        }
        self.call(SteerAgentSession {
            machine_id,
            agent,
            session,
            steer,
        })
        .await
    }

    pub async fn answer_coding(
        &self,
        machine_id: String,
        agent: String,
        session: String,
        question: u32,
        reply: String,
    ) -> Result<(), BeepError> {
        if let Some(mesh) = self.mesh().await? {
            return mesh
                .order(
                    &machine_id,
                    &agent,
                    &session,
                    AgentRequest::Answer { question, reply },
                )
                .await;
        }
        self.call(AnswerAgentQuestion {
            machine_id,
            agent,
            session,
            question,
            reply,
        })
        .await
    }

    pub async fn all_terminals(&self) -> Result<Vec<Terminal>, BeepError> {
        if let Some(mesh) = self.mesh().await? {
            return Ok(mesh.all_terminals().await);
        }
        self.call(ListAllTerminals {}).await
    }

    pub async fn terminal_output(&self, id: String) -> Result<TerminalOutput, BeepError> {
        if let Some(mesh) = self.mesh().await? {
            return mesh.terminal_output(parse_id(&id)?).await;
        }
        self.call(GetTerminal { id: parse_id(&id)? }).await
    }

    pub async fn start_command(&self, start: CommandStart) -> Result<Terminal, BeepError> {
        if let Some(mesh) = self.mesh().await? {
            return mesh.start_command(start).await;
        }
        self.call(OpenTerminal {
            project: start.project,
            machine_id: start.machine_id,
            name: start.name,
            command: start.command,
            cwd: start.cwd,
            cols: 80,
            rows: 24,
        })
        .await
    }

    pub async fn attach_terminal(&self, id: String) -> Result<Arc<TerminalConnection>, BeepError> {
        if let Some(mesh) = self.mesh().await? {
            return mesh.attach_terminal(parse_id(&id)?).await;
        }
        let request = AttachTerminal {
            id: parse_id(&id)?,
            from: 0,
            cols: 80,
            rows: 24,
        };
        let client = self.client()?;
        let (input, output) =
            timed(CALL_TIMEOUT, "attach terminal", client.duplex(&request)).await?;
        Ok(Arc::new(TerminalConnection {
            input: AsyncMutex::new(TerminalInputStream::Server(input)),
            output: AsyncMutex::new(TerminalOutputStream::Server(output)),
        }))
    }

    pub async fn close_terminal(&self, id: String) -> Result<(), BeepError> {
        if let Some(mesh) = self.mesh().await? {
            return mesh.close_terminal(parse_id(&id)?).await;
        }
        self.call(CloseTerminal { id: parse_id(&id)? }).await
    }
}

/// The screen owns both halves and drops them when it leaves. No background task owns a terminal.
#[derive(uniffi::Object)]
pub struct TerminalConnection {
    pub(super) input: AsyncMutex<TerminalInputStream>,
    pub(super) output: AsyncMutex<TerminalOutputStream>,
}

pub(super) enum TerminalInputStream {
    Server(Uplink<TerminalInput>),
    Mesh(LinkTx),
    ViewOnly,
}

pub(super) enum TerminalOutputStream {
    Server(Replies<TerminalEvent>),
    Mesh(LinkRx),
    ViewOnly(bool),
}

#[derive(uniffi::Record)]
pub struct MobileTerminalFrame {
    pub data: Vec<u8>,
    pub view_only: bool,
    pub ended: bool,
}

#[uniffi::export(async_runtime = "tokio")]
impl TerminalConnection {
    /// Next PTY output, or whether the terminal is read-only or ended.
    pub async fn next(&self) -> Result<Option<MobileTerminalFrame>, BeepError> {
        let mut output = self.output.lock().await;
        let event = match &mut *output {
            TerminalOutputStream::Server(output) => output.next().await?,
            TerminalOutputStream::Mesh(output) => loop {
                let Some(reply) = output.recv::<TerminalReply>().await? else {
                    break None;
                };
                match terminals::heard(reply) {
                    Heard::Show(event) => break Some(event),
                    Heard::End(state) => {
                        break Some(TerminalEvent::Ended { state });
                    }
                    Heard::Ignore => {}
                }
            },
            TerminalOutputStream::ViewOnly(pending) => {
                std::mem::take(pending).then_some(TerminalEvent::ViewOnly {})
            }
        };
        Ok(event.map(|event| match event {
            TerminalEvent::Output { data, .. } => MobileTerminalFrame {
                data: data.into_vec(),
                view_only: false,
                ended: false,
            },
            TerminalEvent::ViewOnly {} => MobileTerminalFrame {
                data: Vec::new(),
                view_only: true,
                ended: false,
            },
            TerminalEvent::Ended { .. } => MobileTerminalFrame {
                data: Vec::new(),
                view_only: false,
                ended: true,
            },
        }))
    }

    pub async fn keys(&self, text: String) -> Result<(), BeepError> {
        if text.len() > MAX_TERMINAL_INPUT {
            return Err(BeepError::BadRequest {
                reason: "Terminal input is too long".into(),
            });
        }
        let mut input = self.input.lock().await;
        match &mut *input {
            TerminalInputStream::Server(input) => {
                timed(
                    CALL_TIMEOUT,
                    "terminal input",
                    input.send(&TerminalInput::Keys {
                        data: text.into_bytes().into(),
                    }),
                )
                .await
            }
            TerminalInputStream::Mesh(input) => {
                timed(
                    CALL_TIMEOUT,
                    "terminal input",
                    input.send(&TerminalRequest::Input {
                        data: text.into_bytes().into(),
                    }),
                )
                .await
            }
            TerminalInputStream::ViewOnly => Err(crate::bad_request("This terminal is view only.")),
        }
    }
}

fn parse_id(id: &str) -> Result<Uuid, BeepError> {
    Uuid::parse_str(id).map_err(|_| BeepError::BadRequest {
        reason: "Invalid workspace identifier".into(),
    })
}

impl From<ProjectRunDetail> for MobileRunDetail {
    fn from(detail: ProjectRunDetail) -> Self {
        Self {
            run: detail.run.into(),
            jobs: detail
                .jobs
                .into_iter()
                .map(|job| MobileJob {
                    name: job.name,
                    state: job.state,
                    steps: job
                        .steps
                        .into_iter()
                        .map(|step| MobileStep {
                            name: step.name,
                            state: step.state,
                        })
                        .collect(),
                })
                .collect(),
            log: detail.log.map(|log| log.text),
            error: detail.error,
        }
    }
}

#[uniffi::export(async_runtime = "tokio")]
impl BeepClient {
    pub async fn projects_on_top(&self) -> Result<Vec<String>, BeepError> {
        if let Some(mesh) = self.mesh().await? {
            return Ok(mesh.desk().await?.on_top());
        }
        self.call(ListProjectsOnTop {}).await
    }

    pub async fn pin_project(&self, project: String, pinned: bool) -> Result<(), BeepError> {
        if let Some(mesh) = self.mesh().await? {
            return Ok(mesh.desk().await?.pin(&project, pinned).await?);
        }
        self.call(PinProjectToTop { project, pinned }).await
    }

    pub async fn remove_project(&self, project: String) -> Result<(), BeepError> {
        if let Some(mesh) = self.mesh().await? {
            return Ok(mesh.desk().await?.remove(&project).await?);
        }
        self.call(RemoveProject { project }).await
    }

    pub async fn pause_project_sync(
        &self,
        project: String,
        paused: bool,
    ) -> Result<ProjectSyncDetail, BeepError> {
        if let Some(mesh) = self.mesh().await? {
            return Ok(mesh.desk().await?.pause_sync(&project, paused).await?);
        }
        self.call(PauseProjectSync { project, paused }).await
    }
}
