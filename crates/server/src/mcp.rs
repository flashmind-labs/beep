//! Fastmail MCP integration for the beep server.
//!
//! Connects to Fastmail's official MCP server using a bearer token and
//! exposes its tools (email, contacts, calendar) to the agent.

use anyhow::Result;
use flashmind::tools::mcp::{McpRegistry, McpServerConfig, make_mcp_tool_wrappers};
use flashmind::types::ToolRegistry;
use std::sync::Arc;

/// In-memory config provider for a single pre-configured MCP server.
struct SingleServerProvider {
    config: tokio::sync::Mutex<McpServerConfig>,
}

#[async_trait::async_trait]
impl flashmind::tools::mcp::McpConfigProvider for SingleServerProvider {
    async fn list_configs(&self) -> Result<Vec<McpServerConfig>> {
        Ok(vec![self.config.lock().await.clone()])
    }
    async fn save_config(&self, config: &McpServerConfig) -> Result<()> {
        *self.config.lock().await = config.clone();
        Ok(())
    }
    async fn delete_config(&self, _name: &str) -> Result<()> {
        Ok(())
    }
    async fn save_credentials(&self, _name: &str, credentials: &serde_json::Value) -> Result<()> {
        self.config.lock().await.credentials = Some(credentials.clone());
        Ok(())
    }
    async fn load_credentials(&self, _name: &str) -> Result<Option<serde_json::Value>> {
        Ok(self.config.lock().await.credentials.clone())
    }
}

/// Create an MCP registry pre-configured for Fastmail.
pub fn fastmail_registry(token: String) -> McpRegistry {
    let config = McpServerConfig {
        name: "fastmail".into(),
        url: Some("https://api.fastmail.com/mcp".into()),
        bearer_token: Some(token),
        ..Default::default()
    };
    let provider: Arc<dyn flashmind::tools::mcp::McpConfigProvider> =
        Arc::new(SingleServerProvider {
            config: tokio::sync::Mutex::new(config),
        });
    McpRegistry::new(provider, None)
}

/// Load saved MCP configs, connect, and register tools into the agent's registry.
pub async fn register_mcp_tools(mcp: &McpRegistry, registry: &mut ToolRegistry) {
    mcp.load_saved().await;
    for op in mcp.drain_pending_ops() {
        if let flashmind::tools::mcp::McpToolOp::Register {
            server_name,
            tool_defs,
            restricted,
            session_approved,
        } = op
        {
            let wrappers =
                make_mcp_tool_wrappers(mcp, &server_name, &tool_defs, restricted, session_approved);
            for wrapper in wrappers {
                registry.register(wrapper);
            }
        }
    }
}
