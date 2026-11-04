//! Mobile access to existing workspace requests. No wire types live here.

use crate::{AsyncMutex, BeepClient, BeepError, CALL_TIMEOUT, timed};
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
    AttachTerminal, MAX_TERMINAL_INPUT, OpenTerminal, TerminalEvent, TerminalInput,
    client::{Replies, Uplink},
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
        self.call(GetInstructions {}).await
    }

    pub async fn instructions_history(&self) -> Result<Vec<InstructionsVersion>, BeepError> {
        self.call(ListInstructionsVersions {}).await
    }

    pub async fn save_instructions(
        &self,
        text: String,
        base: Option<u32>,
    ) -> Result<InstructionsVersion, BeepError> {
        self.call(SaveInstructions {
            text,
            base,
            note: String::new(),
        })
        .await
    }

    pub async fn read_agent_files(&self) -> Result<Vec<MachineAgentFiles>, BeepError> {
        self.call(ReadAgentFiles {}).await
    }

    pub async fn coding_usage(&self) -> Result<MobileUsage, BeepError> {
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
        self.call(GetAgentSessionUsage {
            machine_id,
            agent,
            session,
        })
        .await
    }

    pub async fn list_projects(&self) -> Result<Vec<ProjectAgent>, BeepError> {
        self.call(ListProjects {}).await
    }

    pub async fn list_imported_projects(&self) -> Result<Vec<ImportedProject>, BeepError> {
        self.call(ListImportedProjects {}).await
    }

    pub async fn project_agent(&self, project: String) -> Result<ProjectAgent, BeepError> {
        self.call(GetProjectAgent { project }).await
    }

    pub async fn import_project(
        &self,
        repo: String,
        machine_id: String,
        folder: Option<String>,
    ) -> Result<ImportedProject, BeepError> {
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
        self.call(ListProjectMachines { project }).await
    }

    pub async fn project_branches(&self, project: String) -> Result<ProjectBranches, BeepError> {
        self.call(ListProjectBranches { project }).await
    }

    pub async fn thread_detail(
        &self,
        project_id: String,
        number: u64,
    ) -> Result<ThreadDetail, BeepError> {
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
        let detail = self
            .call(GetProjectRun {
                project_id: parse_id(&project_id)?,
                run_id,
            })
            .await?;
        Ok(MobileRunDetail {
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
        })
    }

    pub async fn github_accounts(&self) -> Result<Vec<GitHubAccount>, BeepError> {
        self.call(ListGitHubAccounts {}).await
    }

    pub async fn coding_hosts(&self) -> Result<Vec<AgentHost>, BeepError> {
        self.call(ListAgentHosts {}).await
    }

    pub async fn coding_sessions(
        &self,
        project: Option<String>,
    ) -> Result<Vec<MachineSession>, BeepError> {
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
        self.call(GetAgentSession {
            machine_id,
            agent,
            session,
            last: 200,
        })
        .await
    }

    pub async fn coding_models(
        &self,
        machine_id: String,
        agent: String,
    ) -> Result<AgentModels, BeepError> {
        self.call(ListAgentModels { machine_id, agent }).await
    }

    pub async fn start_coding(&self, start: CodingStart) -> Result<StartedAgent, BeepError> {
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
        self.call(ListAllTerminals {}).await
    }

    pub async fn terminal_output(&self, id: String) -> Result<TerminalOutput, BeepError> {
        self.call(GetTerminal { id: parse_id(&id)? }).await
    }

    pub async fn start_command(&self, start: CommandStart) -> Result<Terminal, BeepError> {
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
        let request = AttachTerminal {
            id: parse_id(&id)?,
            from: 0,
            cols: 80,
            rows: 24,
        };
        let client = self.client().await?;
        let (input, output) =
            timed(CALL_TIMEOUT, "attach terminal", client.duplex(&request)).await?;
        Ok(Arc::new(TerminalConnection {
            input: AsyncMutex::new(input),
            output: AsyncMutex::new(output),
        }))
    }

    pub async fn close_terminal(&self, id: String) -> Result<(), BeepError> {
        self.call(CloseTerminal { id: parse_id(&id)? }).await
    }
}

/// The screen owns both halves and drops them when it leaves. No background task owns a terminal.
#[derive(uniffi::Object)]
pub struct TerminalConnection {
    input: AsyncMutex<Uplink<TerminalInput>>,
    output: AsyncMutex<Replies<TerminalEvent>>,
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
        let event = self.output.lock().await.next().await?;
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
        timed(
            CALL_TIMEOUT,
            "terminal input",
            input.send(&TerminalInput::Keys {
                data: text.into_bytes().into(),
            }),
        )
        .await
    }
}

fn parse_id(id: &str) -> Result<Uuid, BeepError> {
    Uuid::parse_str(id).map_err(|_| BeepError::BadRequest {
        reason: "Invalid workspace identifier".into(),
    })
}
