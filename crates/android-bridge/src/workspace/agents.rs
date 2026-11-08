use crate::{BeepClient, BeepError};
use beep_proto::{
    AgentCheckoutPlan, AgentEffort, AgentHost, AgentModels, AgentSessionView, AgentSteer,
    AnswerAgentQuestion, ChatApproval, GetAgentSession, ListAgentHosts, ListAgentModels,
    ListAgentSessions, ListPendingApprovals, ListProjectSessions, MachineSession,
    ResumeAgentSession, StartAgentSession, StartedAgent, SteerAgentSession,
    node::agent::Request as AgentRequest,
};

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

#[uniffi::export(async_runtime = "tokio")]
impl BeepClient {
    pub async fn workspace_approvals(&self) -> Result<Vec<ChatApproval>, BeepError> {
        if self.mesh().await?.is_some() {
            return Ok(Vec::new());
        }
        self.call(ListPendingApprovals {}).await
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
}
