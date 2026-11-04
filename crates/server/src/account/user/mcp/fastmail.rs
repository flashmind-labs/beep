//! A user's Fastmail accounts: sign-in, addresses and calls.

use super::{super::User, FLOW_TTL, Flow, fastmail_rows, state_of};
use crate::{
    apps::{
        internal,
        mcp::{
            FASTMAIL, FastmailAccount, MAX_FASTMAIL_ACCOUNTS, check_redirect, fastmail_name,
            identity_address, is_fastmail, pick_account, preset,
        },
    },
    protocol::ApiError,
};
use anyhow::{Result, bail};
use serde_json::{Value, json};
use std::time::Instant;

impl User {
    /// The connected Fastmail accounts, oldest (the default) first.
    pub(crate) async fn fastmail_accounts(&self) -> Result<Vec<FastmailAccount>> {
        Ok(fastmail_rows(&self.mcp_servers().await?))
    }

    /// The server of the Fastmail account named by address or id, or of the first.
    pub(crate) async fn fastmail_server(&self, account: Option<&str>) -> Result<String> {
        let accounts = self.fastmail_accounts().await?;
        match pick_account(&accounts, account) {
            Ok(found) => Ok(found.name.clone()),
            Err(message) => bail!(message),
        }
    }

    /// Calls `tool` on the Fastmail account `account` names, or the first.
    pub(crate) async fn call_fastmail(
        &self,
        account: Option<&str>,
        tool: &str,
        arguments: Value,
    ) -> Result<Value> {
        let server = self.fastmail_server(account).await?;
        self.call_mcp(&server, tool, arguments).await
    }

    /// Reads and saves the missing addresses of `accounts` from their sending identities.
    pub(super) async fn learn_addresses(&self, accounts: &mut [FastmailAccount]) {
        for account in accounts
            .iter_mut()
            .filter(|account| account.address.is_none())
        {
            let Some(address) = self.read_address(&account.name).await else {
                continue;
            };
            if let Err(error) = self
                .store
                .set_mcp_address(&self.key, &account.name, Some(&address))
                .await
            {
                log::warn!("mcp: unable to save the account address: {error:#}");
            }
            account.address = Some(address);
        }
    }

    /// The address Fastmail account `name` sends from, if it answers.
    pub(super) async fn read_address(&self, name: &str) -> Option<String> {
        match self.call_mcp(name, "list_identities", json!({})).await {
            Ok(value) => identity_address(&value),
            Err(error) => {
                log::info!(
                    "mcp: unable to read the account address; server={name} error={error:#}"
                );
                None
            }
        }
    }

    /// The Fastmail accounts as the app lists them; missing addresses are read in the background.
    pub(crate) async fn list_fastmail_accounts(&self) -> Result<Vec<FastmailAccount>> {
        let accounts = self.fastmail_accounts().await?;
        self.learn_addresses_later(&accounts);
        Ok(accounts)
    }

    /// Reads the addresses `accounts` lack in one background task at a time, each read bounded.
    pub(super) fn learn_addresses_later(&self, accounts: &[FastmailAccount]) {
        if accounts.iter().all(|account| account.address.is_some()) {
            return;
        }
        let Some(user) = self.me.upgrade() else {
            return;
        };
        if !self.mcp.start_learning() {
            return;
        }
        tokio::spawn(async move {
            match user.fastmail_accounts().await {
                Ok(mut accounts) => user.learn_addresses(&mut accounts).await,
                Err(error) => log::warn!("mcp: unable to list accounts to learn: {error:#}"),
            }
            user.mcp.done_learning();
        });
    }

    /// Starts a Fastmail sign-in and returns its URL; `add` signs in another account.
    pub async fn start_fastmail_auth(
        &self,
        redirect_uri: &str,
        add: bool,
    ) -> Result<String, ApiError> {
        check_redirect(redirect_uri).map_err(|error| ApiError::bad_request(error.to_string()))?;
        let rows = self
            .mcp_servers()
            .await
            .map_err(internal("list MCP servers"))?;
        let accounts = fastmail_rows(&rows);
        let name = if add || accounts.is_empty() {
            // The first slot without a working login; a half-finished one is reused.
            (1..=MAX_FASTMAIL_ACCOUNTS)
                .map(fastmail_name)
                .find(|name| !accounts.iter().any(|account| &account.name == name))
                .ok_or_else(|| {
                    ApiError::bad_request(format!(
                        "You can connect up to {MAX_FASTMAIL_ACCOUNTS} Fastmail accounts."
                    ))
                })?
        } else {
            accounts[0].name.clone()
        };
        let config = preset(&name).ok_or(ApiError::Internal {})?;
        let registry = self.mcp_registry().await;
        let started = async {
            registry.add(config).await?;
            registry.start_auth(&name, redirect_uri).await
        };
        let auth_url = self
            .mcp
            .bounded(&registry, &name, started)
            .await
            .map_err(internal("start MCP OAuth"))?;
        if let Some(state) = state_of(&auth_url)
            && let Ok(mut flows) = self.mcp_flows.lock()
        {
            flows.retain(|_, flow| flow.started.elapsed() < FLOW_TTL && flow.server != name);
            flows.insert(
                state,
                Flow {
                    server: name,
                    started: Instant::now(),
                },
            );
        }
        Ok(auth_url)
    }

    /// Exchanges the redirect's code and connects its account, replacing an older login to it.
    pub async fn finish_fastmail_auth(
        &self,
        code: &str,
        state: &str,
        issuer: Option<&str>,
    ) -> Result<()> {
        let flow = self
            .mcp_flows
            .lock()
            .ok()
            .and_then(|mut flows| flows.remove(state))
            .filter(|flow| is_fastmail(&flow.server));
        let name = flow.map_or_else(|| FASTMAIL.to_owned(), |flow| flow.server);
        let registry = self.mcp_registry().await;
        let completed = registry.complete_auth(&name, code, state, issuer);
        self.mcp.bounded(&registry, &name, completed).await?;
        // The login may be another account's than the slot had before.
        let address = self.read_address(&name).await;
        self.store
            .set_mcp_address(&self.key, &name, address.as_deref())
            .await?;
        let Some(address) = address else {
            return Ok(());
        };
        let older = self
            .fastmail_accounts()
            .await?
            .into_iter()
            .filter(|account| account.name != name && account.address.as_ref() == Some(&address));
        for account in older {
            log::info!("mcp: an account signed in again, dropping its older login; user={self}");
            self.remove_mcp(&account.name).await?;
        }
        Ok(())
    }

    pub async fn remove_fastmail_account(&self, id: &str) -> Result<(), ApiError> {
        if !is_fastmail(id) {
            return Err(ApiError::NotFound {});
        }
        let removed = self
            .remove_mcp(id)
            .await
            .map_err(internal("remove MCP server"))?;
        if removed {
            Ok(())
        } else {
            Err(ApiError::NotFound {})
        }
    }

    /// Disconnects every Fastmail account. Returns whether there was one.
    pub async fn remove_fastmail(&self) -> Result<bool> {
        let names: Vec<String> = self
            .mcp_servers()
            .await?
            .into_iter()
            .map(|row| row.name)
            .filter(|name| is_fastmail(name))
            .collect();
        let mut existed = false;
        for name in names {
            existed |= self.remove_mcp(&name).await?;
        }
        Ok(existed)
    }
}
