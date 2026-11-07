use super::Connected;
use crate::{BeepError, bad_request, workspace::CodingStart};
use beep_mesh::{
    DeviceStatus, Key, PeerClient,
    machine::{self, Host, listing, sessions},
};
use beep_proto::{
    AgentHost, AgentModels, AgentSessionUsage, AgentSessionView, AgentSteer, MachineSession,
    StartedAgent,
    node::{
        agent::{self, Request},
        harness::HarnessConfig,
        mesh_state::StateValue,
    },
};
use futures::{StreamExt, stream};
use std::time::Duration;

const CONCURRENT: usize = 4;
const ORDER_WAIT: Duration = Duration::from_secs(10);

impl Connected {
    fn host(&self, id: &str) -> Result<Host, BeepError> {
        self.mesh
            .devices()
            .into_iter()
            .find(|device| device.device.id.to_string() == id)
            .map(|device| Host {
                id: id.to_owned(),
                name: device.device.name,
            })
            .ok_or(BeepError::NotFound)
    }

    pub async fn hosts(&self) -> Vec<AgentHost> {
        stream::iter(
            self.mesh
                .devices()
                .into_iter()
                .filter(|device| device.online && device.device.id != self.mesh.me()),
        )
        .map(|device| async move { self.probe(device).await })
        .buffer_unordered(CONCURRENT)
        .filter_map(|host| async { host })
        .collect()
        .await
    }

    async fn probe(&self, device: DeviceStatus) -> Option<AgentHost> {
        let snapshot = self.snapshot(&device.device.id.to_string(), false).await;
        let env = snapshot.env;
        let agents: Vec<_> = snapshot
            .offers
            .iter()
            .filter_map(|offer| {
                let name = agent::agent_of(offer)?;
                let off = self
                    .mesh
                    .get(&Key::Harness(name.into()))
                    .and_then(|bytes| HarnessConfig::from_bytes(&bytes).ok())
                    .is_some_and(|config| !config.enabled);
                (!off).then(|| name.to_owned())
            })
            .collect();
        if agents.is_empty() {
            return None;
        }
        Some(AgentHost {
            machine_id: device.device.id.to_string(),
            machine_name: device.device.name,
            agents,
            workspace: None,
            home: env.as_ref().map(|env| env.home.clone()),
            capabilities: env
                .iter()
                .flat_map(|env| env.capabilities())
                .map(machine::machine_capability)
                .collect(),
        })
    }

    pub async fn sessions(&self, project: Option<&str>) -> Vec<MachineSession> {
        let places = project.map(|name| beep_fleet::places(&self.mesh, name));
        let mut sessions: Vec<_> = stream::iter(self.mesh.devices().into_iter().filter(|device| {
            device.online
                && device.device.id != self.mesh.me()
                && places
                    .as_ref()
                    .is_none_or(|places| places.iter().any(|(id, _)| *id == device.device.id))
        }))
        .map(|device| {
            let folders = places.as_ref().map(|places| {
                places
                    .iter()
                    .filter(|(id, _)| *id == device.device.id)
                    .map(|(_, folder)| folder.clone())
                    .collect::<Vec<_>>()
            });
            async move {
                let host = Host {
                    id: device.device.id.to_string(),
                    name: device.device.name,
                };
                let Ok(client) = self.client(&host.id).await else {
                    return Vec::new();
                };
                let mut current = listing::node_sessions(&client, &host).await;
                if let Some(folders) = folders {
                    current.retain(|session| {
                        folders.iter().any(|folder| in_folder(&session.cwd, folder))
                    });
                    let past = listing::project_sessions(&client, &host, folders).await;
                    for session in past {
                        if !current
                            .iter()
                            .any(|known| known.id == session.id && known.agent == session.agent)
                        {
                            current.push(session);
                        }
                    }
                }
                current
            }
        })
        .buffer_unordered(CONCURRENT)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .flatten()
        .collect();
        sessions.sort_by_key(|session| std::cmp::Reverse(session.last_activity));
        sessions
    }

    pub async fn session(
        &self,
        id: &str,
        agent: &str,
        session: &str,
    ) -> Result<AgentSessionView, BeepError> {
        let client = self.client(id).await?;
        let host = self.host(id)?;
        let (listed, transcript) = tokio::join!(
            sessions::list(&client),
            sessions::transcript(&client, session, 200, true)
        );
        let mut view = if let Some(info) = listed?
            .into_iter()
            .find(|info| info.id == session && info.agent == agent)
        {
            machine::beep_view(&host, info)
        } else {
            let outside = sessions::external(&client, agent, 1, Some(session.into()))
                .await?
                .into_iter()
                .find(|found| found.id == session)
                .ok_or(BeepError::NotFound)?;
            let last_message = outside.last_message.clone();
            AgentSessionView {
                session: machine::external_session(&host, outside),
                last_message,
                plan: vec![],
                files: vec![],
                error: None,
                queued: 0,
                turns: 0,
                entries: vec![],
                earlier: 0,
            }
        };
        let (entries, earlier) = transcript?;
        view.entries = machine::entries_for(entries, true);
        view.earlier = earlier;
        Ok(view)
    }

    pub async fn models(&self, id: &str, agent: &str) -> Result<AgentModels, BeepError> {
        let client = self.client(id).await?;
        Ok(sessions::models(&client, agent)
            .await?
            .map(machine::agent_models)
            .unwrap_or(AgentModels {
                pick: beep_proto::AgentModelPick::Fixed {},
                current: None,
                models: vec![],
            }))
    }

    pub async fn usage(
        &self,
        id: &str,
        agent: &str,
        session: &str,
    ) -> Result<AgentSessionUsage, BeepError> {
        let client = self.client(id).await?;
        let info = sessions::list(&client)
            .await?
            .into_iter()
            .find(|info| info.id == session && info.agent == agent)
            .ok_or(BeepError::NotFound)?;
        Ok(AgentSessionUsage {
            context: info.context,
            tokens: info.tokens,
            cost_usd: info.cost_usd,
        })
    }

    pub async fn start(&self, start: CodingStart) -> Result<StartedAgent, BeepError> {
        if start.prompt.trim().is_empty() {
            return Err(bad_request("Say what the agent should do."));
        }
        let client = self.client(&start.machine_id).await?;
        let first = Request::Start {
            cwd: start.cwd,
            prompt: start.prompt,
            model: machine::picked_model(start.model.as_deref()).map_err(BeepError::from)?,
            effort: machine::picked_effort(&start.agent, start.effort).map_err(BeepError::from)?,
            checkout: machine::checkout_for(
                machine::Makes {
                    checkouts: true,
                    branches: true,
                },
                &self.host(&start.machine_id)?.name,
                start.checkout,
            )
            .map_err(BeepError::from)?,
            images: vec![],
        };
        self.opened(&client, &start.machine_id, &start.agent, first)
            .await
    }

    pub async fn resume(
        &self,
        id: &str,
        agent: &str,
        session: &str,
    ) -> Result<StartedAgent, BeepError> {
        let client = self.client(id).await?;
        self.opened(
            &client,
            id,
            agent,
            Request::Resume {
                session: session.into(),
                prompt: None,
            },
        )
        .await
    }

    async fn opened(
        &self,
        client: &PeerClient,
        id: &str,
        agent: &str,
        first: Request,
    ) -> Result<StartedAgent, BeepError> {
        let opened = machine::open_agent(client, agent, first).await?;
        Ok(StartedAgent {
            session: machine::beep_session(&self.host(id)?, opened.info),
            why: String::new(),
        })
    }

    pub async fn steer(
        &self,
        id: &str,
        agent: &str,
        session: &str,
        steer: AgentSteer,
    ) -> Result<(), BeepError> {
        let request = machine::node_request(steer).map_err(BeepError::from)?;
        self.order(id, agent, session, request).await
    }

    pub async fn order(
        &self,
        id: &str,
        agent: &str,
        session: &str,
        request: Request,
    ) -> Result<(), BeepError> {
        let client = self.client(id).await?;
        let mut opened = machine::open_agent(
            &client,
            agent,
            Request::Attach {
                session: session.into(),
            },
        )
        .await?;
        let fallback = machine::queued_instead(&request);
        let result = machine::order(&mut opened, request, ORDER_WAIT).await;
        match (result, fallback) {
            (Err(error), Some(request)) if error.to_string().contains("unknown request") => {
                machine::order(&mut opened, request, ORDER_WAIT).await?;
            }
            (Err(error), _) if error.is_transport() => return Err(BeepError::Connection {
                reason:
                    "The connection dropped after sending. Check the session before sending again."
                        .into(),
            }),
            (result, _) => result?,
        }
        Ok(())
    }
}

fn in_folder(cwd: &str, folder: &str) -> bool {
    let folder = folder.trim_end_matches('/');
    cwd == folder
        || cwd
            .strip_prefix(folder)
            .is_some_and(|tail| tail.starts_with('/'))
}
