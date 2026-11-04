//! Mobile access to the existing workspace requests.

use crate::{AsyncMutex, BeepClient, BeepError, uuid};
use beep_proto::client::{Replies, Uplink};
use beep_proto::{
    AgentCheckoutPlan, AgentEffort, AgentHost, AgentModels, AgentSessionUsage, AgentSessionView,
    AgentSteer, AnswerAgentQuestion, AttachTerminal, ChatApproval, CloseTerminal, GetAgentSession,
    GetAgentSessionUsage, GetBranchDiff, GetProjectAgent, GetProjectRun, GetProjectSyncDetail,
    GetPullRequestHead, GetTerminal, GetThreadDetail, GetUsageReport, GetWorkDiff, GitHubAccount,
    ImportProject, ImportedProject, ListAgentHosts, ListAgentModels, ListAgentSessions,
    ListAllTerminals, ListGitHubAccounts, ListImportedProjects, ListPendingApprovals,
    ListProjectBranches, ListProjectIssues, ListProjectMachines, ListProjectPullRequests,
    ListProjectRuns, ListProjectSessions, ListProjects, MachineSession, OpenTerminal, ProjectAgent,
    ProjectBranches, ProjectIssue, ProjectMachine, ProjectPullRequest, ProjectSyncDetail,
    PullRequestHead, ResumeAgentSession, RunState, StartAgentSession, StartedAgent,
    SteerAgentSession, Terminal, TerminalEvent, TerminalInput, TerminalOutput, TerminalState,
    ThreadDetail, WorkDiff, WorkflowRun,
};
use serde_bytes::ByteBuf;
use std::{sync::Arc, time::SystemTime};

#[derive(uniffi::Record)]
pub struct MobileAgentStart {
    pub machine_id: Option<String>,
    pub agent: Option<String>,
    pub cwd: Option<String>,
    pub prompt: String,
    pub model: Option<String>,
    pub effort: Option<AgentEffort>,
    pub checkout: Option<AgentCheckoutPlan>,
}

#[derive(uniffi::Record)]
pub struct MobileTerminalStart {
    pub project: String,
    pub machine_id: String,
    pub name: String,
    pub command: String,
    pub cwd: String,
}

// CI replies are wire-only types, so the bridge projects the fields used by mobile.
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
    pub number: u32,
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
pub struct MobilePlanLimit {
    pub agent: String,
    pub minutes: u32,
    pub used_percent: f64,
}

#[derive(uniffi::Record)]
pub struct MobileUsage {
    pub cost_usd: f64,
    pub tokens: u64,
    pub limits: Vec<MobilePlanLimit>,
}

#[uniffi::export(async_runtime = "tokio")]
impl BeepClient {
    pub async fn workspace_approvals(&self) -> Result<Vec<ChatApproval>, BeepError> {
        self.call(ListPendingApprovals {}).await
    }

    pub async fn coding_spend(&self) -> Result<MobileUsage, BeepError> {
        let report = self.call(GetUsageReport { refresh: false }).await?;
        Ok(MobileUsage {
            cost_usd: report.today.total.cost_usd,
            tokens: report.today.tokens,
            limits: report
                .limits
                .into_iter()
                .flat_map(|limit| {
                    limit
                        .windows
                        .into_iter()
                        .map(move |window| MobilePlanLimit {
                            agent: limit.agent.as_str().to_owned(),
                            minutes: window.minutes,
                            used_percent: window.used_percent,
                        })
                })
                .collect(),
        })
    }

    pub async fn imported_projects(&self) -> Result<Vec<ImportedProject>, BeepError> {
        self.call(ListImportedProjects {}).await
    }

    pub async fn project_agents(&self) -> Result<Vec<ProjectAgent>, BeepError> {
        self.call(ListProjects {}).await
    }

    pub async fn project_agent(&self, project: String) -> Result<ProjectAgent, BeepError> {
        self.call(GetProjectAgent { project }).await
    }

    pub async fn import_project(
        &self,
        repo: String,
        machine_id: Option<String>,
        folder: Option<String>,
    ) -> Result<ImportedProject, BeepError> {
        self.call(ImportProject {
            repo,
            machine_id,
            folder,
            worktrees: Vec::new(),
            parent: None,
        })
        .await
    }

    pub async fn github_accounts(&self) -> Result<Vec<GitHubAccount>, BeepError> {
        self.call(ListGitHubAccounts {}).await
    }

    pub async fn project_checkouts(
        &self,
        project: String,
    ) -> Result<Vec<ProjectMachine>, BeepError> {
        self.call(ListProjectMachines { project }).await
    }

    pub async fn project_sync(&self, project: String) -> Result<ProjectSyncDetail, BeepError> {
        self.call(GetProjectSyncDetail { project }).await
    }

    pub async fn project_branches(&self, project: String) -> Result<ProjectBranches, BeepError> {
        self.call(ListProjectBranches { project }).await
    }

    pub async fn project_pulls(
        &self,
        project_id: String,
    ) -> Result<Vec<ProjectPullRequest>, BeepError> {
        self.call(ListProjectPullRequests {
            project_id: uuid(&project_id)?,
        })
        .await
    }

    pub async fn project_issues(&self, project_id: String) -> Result<Vec<ProjectIssue>, BeepError> {
        self.call(ListProjectIssues {
            project_id: uuid(&project_id)?,
        })
        .await
    }

    pub async fn thread_detail(
        &self,
        project_id: String,
        number: u64,
    ) -> Result<ThreadDetail, BeepError> {
        self.call(GetThreadDetail {
            project_id: uuid(&project_id)?,
            number,
        })
        .await
    }

    pub async fn pull_head(
        &self,
        project_id: String,
        number: u64,
    ) -> Result<PullRequestHead, BeepError> {
        self.call(GetPullRequestHead {
            project_id: uuid(&project_id)?,
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

    pub async fn session_diff(&self, session: MachineSession) -> Result<WorkDiff, BeepError> {
        self.call(GetWorkDiff {
            machine_id: session.machine_id,
            root: session.cwd,
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
        let reply = self
            .call(ListProjectRuns {
                project_id: uuid(&project_id)?,
                refresh,
            })
            .await?;
        Ok(MobileRuns {
            runs: reply.runs.into_iter().map(MobileRun::from).collect(),
            error: reply.error,
        })
    }

    pub async fn project_run(
        &self,
        project_id: String,
        run_id: u64,
    ) -> Result<MobileRunDetail, BeepError> {
        let reply = self
            .call(GetProjectRun {
                project_id: uuid(&project_id)?,
                run_id,
            })
            .await?;
        Ok(MobileRunDetail {
            run: reply.run.into(),
            jobs: reply
                .jobs
                .into_iter()
                .map(|job| MobileJob {
                    name: job.name,
                    state: job.state,
                    steps: job
                        .steps
                        .into_iter()
                        .map(|step| MobileStep {
                            number: step.number,
                            name: step.name,
                            state: step.state,
                        })
                        .collect(),
                })
                .collect(),
            log: reply.log.map(|log| log.text),
            error: reply.error,
        })
    }

    pub async fn coding_sessions(&self) -> Result<Vec<MachineSession>, BeepError> {
        self.call(ListAgentSessions {}).await
    }

    pub async fn project_sessions(
        &self,
        project: String,
    ) -> Result<Vec<MachineSession>, BeepError> {
        self.call(ListProjectSessions { project }).await
    }

    pub async fn agent_hosts(&self) -> Result<Vec<AgentHost>, BeepError> {
        self.call(ListAgentHosts {}).await
    }

    pub async fn agent_models(
        &self,
        machine_id: String,
        agent: String,
    ) -> Result<AgentModels, BeepError> {
        self.call(ListAgentModels { machine_id, agent }).await
    }

    pub async fn start_coding_agent(
        &self,
        config: MobileAgentStart,
    ) -> Result<StartedAgent, BeepError> {
        self.call(StartAgentSession {
            machine_id: config.machine_id,
            agent: config.agent,
            cwd: config.cwd,
            prompt: config.prompt,
            needs: Vec::new(),
            chat: None,
            model: config.model,
            effort: config.effort,
            checkout: config.checkout,
        })
        .await
    }

    pub async fn resume_coding_session(
        &self,
        session: MachineSession,
    ) -> Result<StartedAgent, BeepError> {
        self.call(ResumeAgentSession {
            machine_id: session.machine_id,
            agent: session.agent,
            session: session.id,
            prompt: None,
        })
        .await
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

    pub async fn coding_usage(
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

    pub async fn steer_coding_agent(
        &self,
        session: MachineSession,
        steer: AgentSteer,
    ) -> Result<(), BeepError> {
        self.call(SteerAgentSession {
            machine_id: session.machine_id,
            agent: session.agent,
            session: session.id,
            steer,
        })
        .await
    }

    pub async fn answer_coding_agent(
        &self,
        session: MachineSession,
        question: u32,
        reply: String,
    ) -> Result<(), BeepError> {
        self.call(AnswerAgentQuestion {
            machine_id: session.machine_id,
            agent: session.agent,
            session: session.id,
            question,
            reply,
        })
        .await
    }

    pub async fn terminals(&self) -> Result<Vec<Terminal>, BeepError> {
        self.call(ListAllTerminals {}).await
    }

    pub async fn terminal_output(&self, id: String) -> Result<TerminalOutput, BeepError> {
        self.call(GetTerminal { id: uuid(&id)? }).await
    }

    pub async fn start_terminal(&self, config: MobileTerminalStart) -> Result<Terminal, BeepError> {
        self.call(OpenTerminal {
            project: config.project,
            machine_id: config.machine_id,
            name: config.name,
            command: config.command,
            cwd: config.cwd,
            cols: 44,
            rows: 28,
        })
        .await
    }

    pub async fn attach_terminal(&self, id: String) -> Result<Arc<MobileTerminal>, BeepError> {
        let client = self.client().await?;
        let (send, receive) = client
            .duplex(&AttachTerminal {
                id: uuid(&id)?,
                from: 0,
                cols: 44,
                rows: 28,
            })
            .await?;
        Ok(Arc::new(MobileTerminal {
            send: AsyncMutex::new(Some(send)),
            receive: AsyncMutex::new(receive),
        }))
    }

    pub async fn close_terminal(&self, id: String) -> Result<(), BeepError> {
        self.call(CloseTerminal { id: uuid(&id)? }).await
    }
}

#[derive(uniffi::Enum)]
pub enum MobileTerminalEvent {
    Output { data: Vec<u8> },
    Ended { state: TerminalState },
    ViewOnly,
}

#[derive(uniffi::Object)]
pub struct MobileTerminal {
    send: AsyncMutex<Option<Uplink<TerminalInput>>>,
    receive: AsyncMutex<Replies<TerminalEvent>>,
}

#[uniffi::export(async_runtime = "tokio")]
impl MobileTerminal {
    pub async fn next(&self) -> Result<Option<MobileTerminalEvent>, BeepError> {
        Ok(self
            .receive
            .lock()
            .await
            .next()
            .await?
            .map(|event| match event {
                TerminalEvent::Output { data, .. } => MobileTerminalEvent::Output {
                    data: data.into_vec(),
                },
                TerminalEvent::Ended { state } => MobileTerminalEvent::Ended { state },
                TerminalEvent::ViewOnly {} => MobileTerminalEvent::ViewOnly,
            }))
    }

    pub async fn input(&self, text: String) -> Result<(), BeepError> {
        let mut send = self.send.lock().await;
        let send = send
            .as_mut()
            .ok_or_else(|| crate::bad_request("Terminal detached"))?;
        for bytes in text.as_bytes().chunks(beep_proto::MAX_TERMINAL_INPUT) {
            send.send(&TerminalInput::Keys {
                data: ByteBuf::from(bytes.to_vec()),
            })
            .await?;
        }
        Ok(())
    }

    pub async fn detach(&self) -> Result<(), BeepError> {
        if let Some(mut send) = self.send.lock().await.take() {
            send.finish()?;
        }
        Ok(())
    }
}
