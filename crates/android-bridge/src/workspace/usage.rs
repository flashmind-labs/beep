use crate::{BeepClient, BeepError};
use beep_proto::{AgentSessionUsage, GetAgentSessionUsage, GetUsageReport};

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
    pub async fn coding_usage(&self) -> Result<MobileUsage, BeepError> {
        let usage = if let Some(mesh) = self.mesh().await? {
            mesh.desk().await?.usage_report().await?
        } else {
            self.call(GetUsageReport { refresh: false }).await?
        };
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
}
