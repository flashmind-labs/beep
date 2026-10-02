use crate::{
    config::Auth,
    store::{CreateRun, Run, Store},
    tools,
};
use anyhow::Result;
use chrono::Utc;
use flashmind::{
    core::{Agent, CancellationToken, Conversation},
    tools::mcp::McpRegistry,
    types::{AgentEvent, AgentInput, AgentLlmConfig, ContentPart, LlmProvider, Model},
};
use futures::StreamExt;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::HashMap, fmt::Write, sync::Arc, time::Duration};
use tokio::sync::{Mutex, broadcast, oneshot};
use uuid::Uuid;

pub struct App {
    pub store: Store,
    pub auth: Auth,
    pub notifications: broadcast::Sender<String>,
    provider: Arc<dyn LlmProvider>,
    model: Model,
    search: Option<Arc<crate::search::SearchTool>>,
    composio_key: Option<String>,
    mcp: Option<McpRegistry>,
    active: Mutex<HashMap<String, CancellationToken>>,
    device_queries: Mutex<HashMap<String, oneshot::Sender<Value>>>,
}

#[derive(Deserialize)]
pub struct Message {
    pub session_id: Uuid,
    pub request_id: Uuid,
    pub message: String,
    #[serde(default)]
    pub cron_id: Option<String>,
    #[serde(default)]
    pub image: Option<String>,
}

impl App {
    pub fn new(
        store: Store,
        auth: Auth,
        provider: Arc<dyn LlmProvider>,
        model: Model,
        search: Option<Arc<crate::search::SearchTool>>,
        composio_key: Option<String>,
        mcp: Option<McpRegistry>,
    ) -> Arc<Self> {
        let (notifications, _) = broadcast::channel(64);
        Arc::new(Self {
            store,
            auth,
            notifications,
            provider,
            model,
            search,
            composio_key,
            mcp,
            active: Mutex::new(HashMap::new()),
            device_queries: Mutex::new(HashMap::new()),
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
            let is_cron = message.cron_id.is_some();
            let image = message.image.clone();
            tokio::spawn(async move {
                if let Err(error) = app
                    .execute(
                        &user,
                        &run,
                        &message.message,
                        image.as_deref(),
                        is_cron,
                        cancel,
                    )
                    .await
                {
                    log::error!("run failed: {error}; run_id={}", run.id);
                    if let Err(error) = app.store.finish(&user, &run, None, "failed").await {
                        log::error!("unable to persist failure: {error}; run_id={}", run.id);
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

    /// Send a query to the connected device and wait for its response.
    ///
    /// Emits a `device_query` event on the run's event stream so the client
    /// picks it up, then blocks until the device POSTs the result back (or the
    /// timeout expires).
    pub async fn device_query(
        &self,
        run_id: &str,
        query: Value,
        timeout_secs: u64,
    ) -> Result<Value> {
        let query_id = Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();
        self.device_queries
            .lock()
            .await
            .insert(query_id.clone(), tx);
        self.store
            .append(
                run_id,
                json!({
                    "type": "device_query",
                    "query_id": query_id,
                    "query": query,
                }),
            )
            .await?;
        let result = tokio::time::timeout(Duration::from_secs(timeout_secs), rx).await;
        // Clean up on timeout or channel close.
        self.device_queries.lock().await.remove(&query_id);
        match result {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(_)) => anyhow::bail!("device query channel closed without a response"),
            Err(_) => anyhow::bail!("device query timed out after {timeout_secs}s"),
        }
    }

    /// Called by the device-result endpoint to deliver a response for a pending
    /// device query. Returns `true` if the query_id was found and resolved.
    pub async fn resolve_device_query(&self, query_id: &str, result: Value) -> bool {
        if let Some(tx) = self.device_queries.lock().await.remove(query_id) {
            tx.send(result).is_ok()
        } else {
            false
        }
    }

    pub async fn notify_run_complete(&self, user: &str, run: &Run, status: &str) {
        // Build a title from the session's first message (truncated).
        let title = match self.store.sessions(user).await {
            Ok(sessions) => sessions
                .iter()
                .find(|s| s["id"].as_str() == Some(&run.session_id))
                .and_then(|s| s["title"].as_str().map(|t| t.to_owned()))
                .unwrap_or_else(|| "Run finished".to_owned()),
            Err(_) => "Run finished".to_owned(),
        };
        let body = match status {
            "completed" => "Your request completed successfully.".to_owned(),
            "cancelled" => "The run was cancelled.".to_owned(),
            "timed_out" => "The run timed out.".to_owned(),
            other => format!("Run ended with status: {other}"),
        };
        let event = json!({
            "type": "notification",
            "user": user,
            "title": title,
            "body": body,
            "conversation_id": run.session_id,
        });
        // Best-effort: ignore send errors (no active receivers).
        let _ = self.notifications.send(event.to_string());
        if let Ok(tokens) = self.store.device_tokens(user).await
            && !tokens.is_empty()
        {
            log::info!(
                "would send push; user={user} run_id={} status={status} devices={}",
                run.id,
                tokens.len()
            );
        }
    }

    async fn build_context(&self, user: &str) -> Option<String> {
        let mut ctx = String::from("[Context -- do not repeat to user]\n");
        let mut has_content = false;

        // Time with user's timezone
        if let Ok(tz_name) = self.store.get_timezone(user).await {
            let now = if let Ok(tz) = tz_name.parse::<chrono_tz::Tz>() {
                Utc::now()
                    .with_timezone(&tz)
                    .format("%Y-%m-%d %H:%M %Z")
                    .to_string()
            } else {
                Utc::now().format("%Y-%m-%d %H:%M UTC").to_string()
            };
            let _ = writeln!(ctx, "Time: {now}");
            has_content = true;
        }

        // Location
        if let Ok(Some(loc)) = self.store.get_location(user).await
            && let (Some(lat), Some(lon)) = (loc["latitude"].as_f64(), loc["longitude"].as_f64())
        {
            let ns = if lat >= 0.0 { "N" } else { "S" };
            let ew = if lon >= 0.0 { "E" } else { "W" };
            let _ = writeln!(
                ctx,
                "Location: {:.4} {ns}, {:.4} {ew}",
                lat.abs(),
                lon.abs()
            );
            has_content = true;
        }

        // Memories (most recent 15)
        if let Ok(memories) = self.store.list_memories(user).await
            && !memories.is_empty()
        {
            let _ = writeln!(ctx, "Memories:");
            for mem in memories.iter().take(15) {
                if let (Some(key), Some(content)) = (mem["key"].as_str(), mem["content"].as_str()) {
                    let _ = writeln!(ctx, "- {key}: {content}");
                }
            }
            has_content = true;
        }

        has_content.then_some(ctx)
    }

    /// Parse a `data:image/jpeg;base64,...` URI into a `ContentPart::Image`.
    fn parse_image_data_uri(uri: &str) -> Option<ContentPart> {
        let rest = uri.strip_prefix("data:")?;
        let (header, data) = rest.split_once(",")?;
        let media_type = header.strip_suffix(";base64")?;
        if !media_type.starts_with("image/") {
            return None;
        }
        Some(ContentPart::Image {
            media_type: media_type.to_owned(),
            data: data.to_owned(),
        })
    }

    async fn execute(
        self: &Arc<Self>,
        user: &str,
        run: &Run,
        message: &str,
        image: Option<&str>,
        is_cron: bool,
        cancel: CancellationToken,
    ) -> Result<()> {
        let mut conversation = self
            .store
            .conversation(user, &run.session_id)
            .await?
            .unwrap_or_else(Conversation::new);
        // Always set the server-owned prompt, including when loading an older conversation.
        let base_prompt = include_str!("../prompts/assistant.md");
        let system_prompt = match self.build_context(user).await {
            Some(ctx) => format!("{base_prompt}\n\n{ctx}"),
            None => base_prompt.to_owned(),
        };
        conversation.set_system(&system_prompt);
        let mut llm = AgentLlmConfig::new(self.model.clone()).with_user(user);
        llm.max_tokens = Some(4096);
        let mut registry = tools::registry(self.clone(), &run.id, user);
        if let Some(search) = &self.search {
            registry.register(search.clone());
        }
        if let Some(key) = &self.composio_key {
            use flashmind::tools::composio::{ComposioClient, make_composio_tool_wrappers};
            let client = Arc::new(
                ComposioClient::new(key.clone(), None, None).with_entity_id(user.to_owned()),
            );
            if let Ok(defs) = client.list_tools(&[]).await {
                for tool in make_composio_tool_wrappers(&client, &defs) {
                    registry.register(tool);
                }
            }
        }
        if let Some(mcp) = &self.mcp {
            crate::mcp::register_mcp_tools(mcp, &mut registry).await;
        }
        let mut agent = Agent::builder(self.provider.clone())
            .llm(llm)
            .tools(registry)
            .build_sync();
        let input = match image.and_then(Self::parse_image_data_uri) {
            Some(part) => AgentInput::User {
                content: message.to_owned(),
                context: None,
                parts: Some(vec![part]),
            },
            None => AgentInput::user(message),
        };
        let status = {
            let stream = agent.start(&mut conversation, cancel.clone(), input, None);
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
        if is_cron {
            // Notify registered devices for cron-triggered runs only.
            self.notify_run_complete(user, run, status).await;
            // Log cron completion.
            let _ = self.store.log_cron_finish(&run.id, status, "").await;
        }
        // Persist state and the terminal event together before clients observe completion.
        self.store
            .finish(user, run, Some(&conversation), status)
            .await
    }
}
