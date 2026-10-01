use crate::{
    config::Auth,
    store::{CreateRun, Run, Store},
    tools,
};
use anyhow::Result;
use flashmind::{
    core::{Agent, CancellationToken, Conversation},
    types::{AgentEvent, AgentInput, AgentLlmConfig, LlmProvider, Model},
};
use futures::StreamExt;
use serde::Deserialize;
use serde_json::json;
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::sync::Mutex;
use uuid::Uuid;

pub struct App {
    pub store: Store,
    pub auth: Auth,
    provider: Arc<dyn LlmProvider>,
    model: Model,
    search: Option<Arc<crate::search::SearchTool>>,
    active: Mutex<HashMap<String, CancellationToken>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Message {
    pub session_id: Uuid,
    pub request_id: Uuid,
    pub message: String,
}

impl App {
    pub fn new(
        store: Store,
        auth: Auth,
        provider: Arc<dyn LlmProvider>,
        model: Model,
    ) -> Arc<Self> {
        Self::with_search(store, auth, provider, model, None)
    }

    pub fn with_search(
        store: Store,
        auth: Auth,
        provider: Arc<dyn LlmProvider>,
        model: Model,
        search: Option<Arc<crate::search::SearchTool>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            store,
            auth,
            provider,
            model,
            search,
            active: Mutex::new(HashMap::new()),
        })
    }

    pub async fn submit(self: &Arc<Self>, user: &str, message: Message) -> Result<CreateRun> {
        // Serialize registration with cancellation so a newly created run can always be stopped.
        let mut active = self.active.lock().await;
        let result = self
            .store
            .create_run(
                user,
                &message.session_id.to_string(),
                &message.request_id.to_string(),
                &message.message,
            )
            .await?;
        if let CreateRun::Created(run) = &result {
            let cancel = CancellationToken::new();
            active.insert(run.id.clone(), cancel.clone());
            let (app, user, run) = (self.clone(), user.to_owned(), run.clone());
            tokio::spawn(async move {
                if let Err(error) = app.execute(&user, &run, &message.message, cancel).await {
                    tracing::error!(run_id=%run.id, %error, "run storage/execution failed");
                    if let Err(error) = app.store.finish(&user, &run, None, "failed").await {
                        tracing::error!(run_id=%run.id,%error,"unable to persist failure");
                    }
                }
                app.active.lock().await.remove(&run.id);
            });
        }
        Ok(result)
    }

    pub async fn cancel(&self, id: &str) {
        if let Some(token) = self.active.lock().await.get(id) {
            token.cancel();
        }
    }

    pub async fn shutdown(&self) {
        for token in self.active.lock().await.values() {
            token.cancel();
        }
        let _ = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if self.active.lock().await.is_empty() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await;
    }

    pub async fn notify_run_complete(&self, user: &str, run: &Run, status: &str) {
        if let Ok(tokens) = self.store.device_tokens(user).await {
            if !tokens.is_empty() {
                tracing::info!(user, run_id=%run.id, status, devices=tokens.len(), "would send push notification");
            }
        }
    }

    async fn execute(
        &self,
        user: &str,
        run: &Run,
        message: &str,
        cancel: CancellationToken,
    ) -> Result<()> {
        let mut conversation = self
            .store
            .conversation(user, &run.session_id)
            .await?
            .unwrap_or_else(Conversation::new);
        // Always set the server-owned prompt, including when loading an older conversation.
        conversation.set_system(include_str!("../prompts/assistant.md"));
        let mut llm = AgentLlmConfig::new(self.model.clone()).with_user(user);
        llm.max_tokens = Some(4096);
        let mut registry = tools::registry(self.store.clone(), user);
        if let Some(search) = &self.search {
            registry.register(search.clone());
        }
        let mut agent = Agent::builder(self.provider.clone())
            .llm(llm)
            .tools(registry)
            .build_sync();
        let status = {
            let stream = agent.start(
                &mut conversation,
                cancel.clone(),
                AgentInput::user(message),
                None,
            );
            tokio::pin!(stream);
            let deadline = tokio::time::sleep(Duration::from_secs(180));
            tokio::pin!(deadline);
            let mut tools_started = 0;
            loop {
                let event = tokio::select! {
                    biased;
                    _ = cancel.cancelled() => break "cancelled",
                    _ = &mut deadline => { cancel.cancel(); break "timed_out"; },
                    event = stream.next() => event,
                };
                let data = match event {
                    Some(AgentEvent::TextDelta(delta)) => {
                        json!({"type":"text_delta","delta":delta})
                    }
                    Some(AgentEvent::ToolStart {
                        name,
                        id,
                        humanized,
                    }) => {
                        tools_started += 1;
                        if tools_started > 20 {
                            cancel.cancel();
                            break "tool_limit";
                        }
                        json!({"type":"tool_started","name":name,"call_id":id,"description":humanized})
                    }
                    Some(AgentEvent::ToolResult {
                        name,
                        id,
                        output,
                        success,
                        ..
                    }) => {
                        json!({"type":"tool_result","name":name,"call_id":id,"output":output,"success":success})
                    }
                    Some(AgentEvent::Usage(usage)) => json!({"type":"usage","usage":usage}),
                    Some(AgentEvent::Done(_)) => break "completed",
                    Some(AgentEvent::Error(_)) => {
                        // Provider errors can contain upstream response details. Keep them off the wire.
                        self.store.append(&run.id,json!({"type":"error","message":"The model request failed. Check server provider configuration."})).await?;
                        break "failed";
                    }
                    None => break "failed",
                    _ => continue,
                };
                self.store.append(&run.id, data).await?;
            }
        };
        // Notify registered devices before persisting completion.
        self.notify_run_complete(user, run, status).await;
        // Persist state and the terminal event together before clients observe completion.
        self.store
            .finish(user, run, Some(&conversation), status)
            .await
    }
}
