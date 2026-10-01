//! Composio integration — dynamically expose third-party tools via the Composio REST API.
//! Credentials and upstream errors never reach clients.
use anyhow::{Result, ensure};
use async_trait::async_trait;
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

pub struct ComposioClient {
    client: reqwest::Client,
    key: String,
    base: String,
}

impl ComposioClient {
    pub fn new(key: String) -> Result<Self> {
        ensure!(
            !key.is_empty() && !key.chars().any(char::is_whitespace),
            "invalid Composio API key"
        );
        Ok(Self {
            client: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(30))
                .build()?,
            key,
            base: "https://backend.composio.dev/api/v2".into(),
        })
    }

    pub async fn list_apps(&self) -> Result<Vec<Value>> {
        let response = self
            .client
            .get(format!("{}/apps", self.base))
            .header("x-api-key", &self.key)
            .send()
            .await?;
        let data: Value = response.json().await?;
        Ok(data.get("items").and_then(Value::as_array).cloned().unwrap_or_default())
    }

    pub async fn get_actions(&self, app_name: &str) -> Result<Vec<Value>> {
        let response = self
            .client
            .get(format!("{}/actions", self.base))
            .query(&[("appNames", app_name), ("limit", "20")])
            .header("x-api-key", &self.key)
            .send()
            .await?;
        let data: Value = response.json().await?;
        Ok(data.get("items").and_then(Value::as_array).cloned().unwrap_or_default())
    }

    pub async fn execute_action(
        &self,
        action_id: &str,
        connected_account_id: &str,
        params: &Value,
    ) -> Result<Value> {
        let response = self
            .client
            .post(format!("{}/actions/{}/execute", self.base, action_id))
            .header("x-api-key", &self.key)
            .json(&json!({
                "connectedAccountId": connected_account_id,
                "input": params,
            }))
            .send()
            .await?;
        let data: Value = response.json().await?;
        Ok(data)
    }

    pub async fn list_connected_accounts(&self, entity_id: &str) -> Result<Vec<Value>> {
        let response = self
            .client
            .get(format!("{}/connectedAccounts", self.base))
            .query(&[("user_uuid", entity_id)])
            .header("x-api-key", &self.key)
            .send()
            .await?;
        let data: Value = response.json().await?;
        Ok(data.get("items").and_then(Value::as_array).cloned().unwrap_or_default())
    }

    pub async fn initiate_connection(
        &self,
        app_name: &str,
        entity_id: &str,
        redirect_url: &str,
    ) -> Result<Value> {
        let response = self
            .client
            .post(format!("{}/connectedAccounts", self.base))
            .header("x-api-key", &self.key)
            .json(&json!({
                "integrationId": app_name,
                "userUuid": entity_id,
                "redirectUrl": redirect_url,
            }))
            .send()
            .await?;
        let data: Value = response.json().await?;
        Ok(data)
    }
}

/// A tool that lists available Composio apps and their actions.
pub struct ComposioListTool {
    client: Arc<ComposioClient>,
}

#[async_trait]
impl Tool for ComposioListTool {
    fn name(&self) -> &str {
        "composio_list"
    }
    fn description(&self) -> &str {
        "List available Composio integrations. Returns apps and connected accounts."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "app": {
                    "type": "string",
                    "description": "Optional app name to list actions for (e.g. 'gmail', 'slack', 'github')"
                }
            },
            "additionalProperties": false
        })
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        let app = ctx.args.get("app").and_then(Value::as_str);
        let output = if let Some(app) = app {
            match self.client.get_actions(app).await {
                Ok(actions) => {
                    let summary: Vec<_> = actions
                        .iter()
                        .filter_map(|a| {
                            Some(json!({
                                "id": a.get("name")?.as_str()?,
                                "display_name": a.get("displayName")?.as_str()?,
                                "description": a.get("description")?.as_str().unwrap_or(""),
                            }))
                        })
                        .collect();
                    json!({"app": app, "actions": summary})
                }
                Err(_) => json!({"error": "Could not fetch actions. Check Composio configuration."}),
            }
        } else {
            match self.client.list_apps().await {
                Ok(apps) => {
                    let summary: Vec<_> = apps
                        .iter()
                        .filter_map(|a| {
                            Some(json!({
                                "name": a.get("name")?.as_str()?,
                                "description": a.get("description")?.as_str().unwrap_or(""),
                            }))
                        })
                        .take(50)
                        .collect();
                    json!({"apps": summary})
                }
                Err(_) => json!({"error": "Could not fetch apps. Check Composio configuration."}),
            }
        };
        Ok(ToolResult::success(ctx.tool_call_id, output.to_string()))
    }
}

/// A tool that executes a Composio action.
pub struct ComposioExecuteTool {
    client: Arc<ComposioClient>,
    user: String,
}

#[async_trait]
impl Tool for ComposioExecuteTool {
    fn name(&self) -> &str {
        "composio_execute"
    }
    fn description(&self) -> &str {
        "Execute a Composio action on a connected third-party app (e.g. send email via Gmail, create GitHub issue). First use composio_list to find the action ID."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action_id": {
                    "type": "string",
                    "description": "The Composio action ID (from composio_list)"
                },
                "params": {
                    "type": "object",
                    "description": "Input parameters for the action"
                }
            },
            "required": ["action_id"],
            "additionalProperties": false
        })
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        let action_id = ctx
            .args
            .get("action_id")
            .and_then(Value::as_str)
            .unwrap_or("");
        let params = ctx
            .args
            .get("params")
            .cloned()
            .unwrap_or_else(|| json!({}));

        // Find the user's connected account
        let accounts = match self.client.list_connected_accounts(&self.user).await {
            Ok(a) => a,
            Err(_) => {
                return Ok(ToolResult::failure(
                    ctx.tool_call_id,
                    "Could not list connected accounts. The user may need to connect an app first through the Integrations screen.",
                ));
            }
        };
        let account_id = accounts
            .first()
            .and_then(|a| a.get("id").and_then(Value::as_str));
        let Some(account_id) = account_id else {
            return Ok(ToolResult::failure(
                ctx.tool_call_id,
                "No connected accounts found. The user needs to connect an app first through the Integrations screen.",
            ));
        };

        match self
            .client
            .execute_action(action_id, account_id, &params)
            .await
        {
            Ok(result) => {
                let output = json!({
                    "success": result.get("successfull").and_then(Value::as_bool).unwrap_or(false),
                    "data": result.get("data"),
                });
                Ok(ToolResult::success(ctx.tool_call_id, output.to_string()))
            }
            Err(_) => Ok(ToolResult::failure(
                ctx.tool_call_id,
                "Failed to execute Composio action. Check the action ID and parameters.",
            )),
        }
    }
}

pub fn register(
    registry: &mut flashmind::types::ToolRegistry,
    client: Arc<ComposioClient>,
    user: &str,
) {
    registry.register(Arc::new(ComposioListTool {
        client: client.clone(),
    }));
    registry.register(Arc::new(ComposioExecuteTool {
        client,
        user: user.into(),
    }));
}
