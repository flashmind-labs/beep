//! The user's sealed credentials, GitHub accounts and git logins.

use super::User;
use anyhow::Result;

impl User {
    /// The user's credentials, without their secrets.
    pub async fn credentials(
        &self,
        key: &crate::account::credentials::CredentialKey,
    ) -> Result<Vec<crate::store::CredentialRow>> {
        self.store.credentials(&self.key, key).await
    }

    /// Seals and saves a credential, replacing the one in its slot.
    pub async fn save_credential(
        &self,
        key: &crate::account::credentials::CredentialKey,
        new: crate::store::NewCredential<'_>,
    ) -> Result<crate::store::CredentialRow> {
        self.store.save_credential(&self.key, key, new).await
    }

    /// Seals and saves a GitHub token as the account it signs in as.
    pub async fn save_github_account(
        &self,
        key: &crate::account::credentials::CredentialKey,
        new: crate::store::NewGitHubAccount<'_>,
    ) -> Result<crate::store::GitHubAccountRow> {
        self.store.save_github_account(&self.key, key, new).await
    }

    /// The user's GitHub accounts, the default first.
    pub async fn github_accounts(
        &self,
        key: &crate::account::credentials::CredentialKey,
    ) -> Result<Vec<crate::store::GitHubAccountRow>> {
        self.store.github_accounts(&self.key, key).await
    }

    /// `false` when the user has no GitHub account `id`.
    pub async fn set_default_github(&self, id: &str) -> Result<bool> {
        self.store.set_default_github(&self.key, id).await
    }

    /// The user's coding agents' tokens, without their secrets.
    pub(crate) async fn agent_tokens(
        &self,
        key: &crate::account::credentials::CredentialKey,
    ) -> Result<Vec<crate::store::AgentTokenRow>> {
        self.store.agent_tokens(&self.key, key).await
    }

    /// `false` when the user has no token for `agent`.
    pub(crate) async fn remove_agent_token(
        &self,
        agent: crate::protocol::AgentAccount,
    ) -> Result<bool> {
        self.store.remove_agent_token(&self.key, agent).await
    }

    /// Keeps `account` as who Forgejo credential `id` signs in as; `false` for no such one.
    pub(crate) async fn set_forgejo_account(&self, id: &str, account: &str) -> Result<bool> {
        self.store.set_forgejo_account(&self.key, id, account).await
    }

    /// `false` when the user has no credential `id`.
    pub async fn remove_credential(&self, id: &str) -> Result<bool> {
        self.store.remove_credential(&self.key, id).await
    }

    /// The user's git host tokens, opened.
    pub async fn git_logins(
        &self,
        key: &crate::account::credentials::CredentialKey,
    ) -> Result<Vec<crate::store::GitLogin>> {
        self.store.git_logins(&self.key, key).await
    }

    /// The user's credentials for their machines, opened.
    pub async fn machine_credentials(
        &self,
        key: &crate::account::credentials::CredentialKey,
    ) -> Result<crate::protocol::node::MachineCredentials> {
        let opened = self.store.machine_credentials(&self.key, key).await?;
        Ok(crate::account::credentials::for_machines(opened))
    }
}
