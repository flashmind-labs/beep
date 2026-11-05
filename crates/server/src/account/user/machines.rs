//! The user's devices, keys, node tokens, nodes and coding-agent settings.

use super::User;
use crate::{
    protocol::node::EnvReport,
    store::{LinkKey, NewKey, NewNode, NodeRow, PushDevice},
};
use anyhow::Result;
use chrono::{DateTime, Utc};
use iroh::EndpointId;
use std::time::Duration;

impl User {
    /// Saves a phone's push token, replacing the one it had.
    pub async fn register_device(&self, device: PushDevice<'_>) -> Result<()> {
        self.store.register_device(&self.key, device).await
    }

    /// Removes a phone's push token; `false` when it was not registered.
    pub async fn unregister_device(&self, device_id: &str) -> Result<bool> {
        self.store.unregister_device(&self.key, device_id).await
    }

    /// Links another device's key to this account.
    pub async fn link_key(&self, key: NewKey<'_>) -> Result<LinkKey> {
        self.store.link_key(&self.key, key).await
    }

    /// Keeps a new setup token's hash; returns when it expires.
    pub async fn create_node_token(
        &self,
        hash: &str,
        name: Option<&str>,
        ttl: Duration,
    ) -> Result<DateTime<Utc>> {
        self.store
            .create_node_token(&self.key, hash, name, ttl)
            .await
    }

    /// The key that claimed setup token `hash`: `None` for no token, `Some(None)` while unused.
    pub async fn node_token_claimer(&self, hash: &str) -> Result<Option<Option<String>>> {
        self.store.node_token_claimer(&self.key, hash).await
    }

    /// Revokes the user's unused setup token `hash`; false when there is none.
    pub async fn revoke_node_token(&self, hash: &str) -> Result<bool> {
        self.store.revoke_node_token(&self.key, hash).await
    }

    /// Records that a node said hello.
    pub async fn node_connected(&self, node: NewNode<'_>) -> Result<()> {
        self.store.node_connected(&self.key, node).await
    }

    /// Stores what a node reported about its machine.
    pub async fn save_node_env(&self, node: &EndpointId, env: &EnvReport) -> Result<()> {
        self.store.save_node_env(&self.key, node, env).await
    }

    pub async fn nodes(&self) -> Result<Vec<NodeRow>> {
        self.store.nodes(&self.key).await
    }

    /// The user's settings for the coding agent harnesses on `machine_id`.
    pub(crate) async fn harness_settings(
        &self,
        machine_id: &str,
    ) -> Result<Vec<crate::store::HarnessRow>> {
        self.store.harness_settings(&self.key, machine_id).await
    }

    /// The user's saved settings for harness `agent`, per machine.
    pub(crate) async fn harness_settings_of(
        &self,
        agent: &str,
    ) -> Result<Vec<crate::store::HarnessRow>> {
        self.store.harness_settings_of(&self.key, agent).await
    }

    /// The env vars of the harness `agent` on `machine_id`, opened.
    pub(crate) async fn harness_env(
        &self,
        key: &crate::account::credentials::CredentialKey,
        machine_id: &str,
        agent: &str,
    ) -> Result<Vec<crate::protocol::node::EnvSecret>> {
        self.store
            .harness_env(&self.key, key, machine_id, agent)
            .await
    }

    /// Saves the user's settings for one harness on one machine.
    pub(crate) async fn save_harness_settings(
        &self,
        key: &crate::account::credentials::CredentialKey,
        save: crate::store::SaveHarness<'_>,
    ) -> Result<()> {
        self.store.save_harness_settings(&self.key, key, save).await
    }

    /// The user's flsh settings; `None` until they save some.
    pub(crate) async fn flsh_settings(&self) -> Result<Option<crate::protocol::FlshSettings>> {
        self.store.flsh_settings(&self.key).await
    }

    /// Saves the user's flsh settings in place of the ones before.
    pub(crate) async fn save_flsh_settings(
        &self,
        settings: &crate::protocol::FlshSettings,
    ) -> Result<()> {
        self.store.save_flsh_settings(&self.key, settings).await
    }

    /// Remembers that `device` started `on`, so its alerts go there.
    pub(crate) async fn record_agent_start(
        &self,
        on: crate::store::SessionOn<'_>,
        device: &EndpointId,
    ) -> Result<()> {
        self.store.record_agent_start(&self.key, on, device).await
    }

    /// The device that started `on` through Beep, if any.
    pub(crate) async fn agent_starter(
        &self,
        on: crate::store::SessionOn<'_>,
    ) -> Result<Option<EndpointId>> {
        self.store.agent_starter(&self.key, on).await
    }

    /// The harnesses the user turned off, on any machine.
    pub(crate) async fn off_harnesses(&self) -> Result<Vec<crate::store::OffHarness>> {
        self.store.off_harnesses(&self.key).await
    }

    /// The harness settings for `machine_id` as the node takes them, without vars that fail to open.
    pub(crate) async fn agent_settings(
        &self,
        key: &crate::account::credentials::CredentialKey,
        machine_id: &str,
    ) -> Result<crate::protocol::node::harness::AgentSettings> {
        use crate::protocol::node::harness::{AgentSettings, HarnessConfig};
        let mut harnesses = Vec::new();
        for row in self.harness_settings(machine_id).await? {
            let env = if row.env_names.is_empty() {
                Vec::new()
            } else {
                match self.harness_env(key, machine_id, &row.agent).await {
                    Ok(env) => env,
                    Err(e) => {
                        log::warn!(
                            "harness env vars not opened; agent={} error={e:#}",
                            row.agent
                        );
                        Vec::new()
                    }
                }
            };
            let text = |value: String| Some(value).filter(|value| !value.is_empty());
            harnesses.push(HarnessConfig {
                agent: row.agent,
                enabled: row.enabled,
                program: text(row.program),
                home: text(row.home),
                args: row.args,
                env,
                compact: None,
            });
        }
        // Settings that no longer read must not hold back the harnesses'
        // own: the node then leaves flsh's config as it is.
        let flsh = match self.flsh_settings().await {
            Ok(flsh) => flsh,
            Err(e) => {
                log::warn!("flsh settings not sent; error={e:#}");
                None
            }
        };
        Ok(AgentSettings { harnesses, flsh })
    }
}
