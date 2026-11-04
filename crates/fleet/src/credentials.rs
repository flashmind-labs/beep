//! The user's credentials, kept in the mesh as `credential/<id>`: the values the nodes read
//! (`Credential::Env` and `Credential::Forgejo`), with everything else the desktop shows derived
//! from the key and the value. An id is `<kind>.<saved_ms>.<name>`: `gh.<ms>.<login>` for a
//! GitHub account, whose `:<owner>` siblings carry the same token for each owner it serves,
//! `forgejo.<ms>.<host>`, `api.<ms>.<provider>` and `agent.<ms>.<agent>`.
//!
//! A secret never reaches a log: [`SecretText`] and [`SecretString`] print nothing.

use crate::{
    desk::Desk,
    error::{FleetError, Result},
    peer::probe,
    records::{WRITES, now_ms, time},
};
use beep_mesh::Key;
use beep_projects::{
    credential_check::{
        Checked, Checker,
        machines::{MAX_MACHINES, Reach, check_from},
    },
    credentials::{GitHubLogin, Opened, Slot, github_for, last4},
    github_owner_var,
    model::ForgejoUrl,
};
use beep_proto::{
    AgentAccount, AgentAccounts, AgentMachine, AgentToken, ApiProvider, Credential,
    CredentialCheck, CredentialKind, GITHUB_ENV_VAR, GitHubAccount, SecretText,
    node::{EnvSecret, ForgejoLogin, mesh_state::Credential as Value},
};
use iroh::EndpointId;
use secrecy::{ExposeSecret, SecretString};
use std::{collections::BTreeMap, str::FromStr};

/// The longest secret taken.
const MAX_SECRET: usize = 4096;
const PREFIX: &str = "credential/";

const GITHUB: &str = "gh";
const FORGEJO: &str = "forgejo";
const API: &str = "api";
const AGENT: &str = "agent";

/// A saved credential, read back from its keys.
struct Saved {
    /// The id of its main key.
    id: String,
    saved_ms: u64,
    slot: Slot,
    secret: SecretString,
    /// For a GitHub account.
    github: Option<GitHubLogin>,
}

impl Saved {
    fn opened(&self) -> Opened {
        Opened {
            slot: self.slot.clone(),
            secret: self.secret.clone(),
            github: self.github.clone(),
        }
    }
}

fn make_id(kind: &str, saved_ms: u64, name: &str) -> String {
    format!("{kind}.{saved_ms}.{name}")
}

/// `(kind, saved_ms, name)` of an id.
fn parse_id(id: &str) -> Option<(&str, u64, &str)> {
    let mut parts = id.splitn(3, '.');
    let kind = parts.next()?;
    let saved = parts.next()?.parse().ok()?;
    Some((kind, saved, parts.next()?))
}

fn key(id: &str) -> Key {
    Key::Credential(id.to_owned())
}

fn text(secret: &SecretText) -> SecretString {
    SecretString::from(secret.expose())
}

fn env(name: &str, secret: &SecretString) -> Value {
    Value::Env(EnvSecret {
        name: name.to_owned(),
        value: SecretText::new(secret.expose_secret()),
    })
}

/// `secret` trimmed, refused when it does not look like a token.
fn trimmed(secret: &SecretText) -> Result<SecretString> {
    let secret = SecretString::from(secret.expose().trim());
    let text = secret.expose_secret();
    if text.is_empty() {
        return Err(FleetError::refused("Paste the token or key first."));
    }
    if text.len() > MAX_SECRET || text.chars().any(char::is_whitespace) {
        return Err(FleetError::refused(
            "That does not look like a token or key: it has spaces or is too long.",
        ));
    }
    Ok(secret)
}

impl Desk {
    /// Every saved credential, the GitHub accounts first.
    fn saved(&self) -> Vec<Saved> {
        let mut found: Vec<(String, Value)> = crate::records::scan::<Value>(&self.mesh, PREFIX)
            .into_iter()
            .filter_map(|(key, value)| match key {
                Key::Credential(id) => Some((id, value)),
                _ => None,
            })
            .collect();
        found.sort_by(|a, b| a.0.cmp(&b.0));
        let mut out: Vec<Saved> = Vec::new();
        let mut orgs: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (id, value) in &found {
            if let Some((main, owner)) = id.split_once(':') {
                orgs.entry(main.to_owned())
                    .or_default()
                    .push(owner.to_owned());
                continue;
            }
            let Some((kind, saved_ms, name)) = parse_id(id) else {
                continue;
            };
            let saved = match (kind, value) {
                (GITHUB, Value::Env(secret)) => Saved {
                    id: id.clone(),
                    saved_ms,
                    slot: Slot::GitHub,
                    secret: text(&secret.value),
                    github: Some(GitHubLogin {
                        login: name.to_owned(),
                        orgs: Vec::new(),
                        default: secret.name == GITHUB_ENV_VAR,
                    }),
                },
                (FORGEJO, Value::Forgejo(login)) => {
                    let Ok(url) = login.url.parse::<ForgejoUrl>() else {
                        continue;
                    };
                    Saved {
                        id: id.clone(),
                        saved_ms,
                        slot: Slot::Forgejo(url),
                        secret: text(&login.token),
                        github: None,
                    }
                }
                (API, Value::Env(secret)) => {
                    let Ok(provider) = ApiProvider::from_str(name) else {
                        continue;
                    };
                    Saved {
                        id: id.clone(),
                        saved_ms,
                        slot: Slot::ApiKey(provider),
                        secret: text(&secret.value),
                        github: None,
                    }
                }
                (AGENT, Value::Env(secret)) => {
                    let Ok(agent) = AgentAccount::from_str(name) else {
                        continue;
                    };
                    Saved {
                        id: id.clone(),
                        saved_ms,
                        slot: Slot::AgentToken(agent),
                        secret: text(&secret.value),
                        github: None,
                    }
                }
                _ => continue,
            };
            out.push(saved);
        }
        // The owner keys name the orgs: an owner other than the login.
        for saved in &mut out {
            let Some(login) = saved.github.as_mut() else {
                continue;
            };
            let mut owners = orgs.remove(&saved.id).unwrap_or_default();
            owners.retain(|owner| !owner.eq_ignore_ascii_case(&login.login));
            login.orgs = owners;
        }
        out.sort_by_key(|saved| (saved.slot != Slot::GitHub, saved.saved_ms));
        out
    }

    /// The credentials as git hosts and clones use them.
    pub(crate) fn opened(&self) -> Vec<Opened> {
        self.saved().iter().map(Saved::opened).collect()
    }

    /// The GitHub accounts, the default first.
    pub fn github_accounts(&self) -> Vec<GitHubAccount> {
        let mut accounts: Vec<GitHubAccount> = self
            .saved()
            .into_iter()
            .filter_map(|saved| {
                let login = saved.github?;
                Some(GitHubAccount {
                    id: saved.id,
                    login: login.login,
                    orgs: login.orgs,
                    default: login.default,
                    last4: last4(saved.secret.expose_secret()),
                    created_at: time(saved.saved_ms),
                    unreadable: false,
                })
            })
            .collect();
        accounts.sort_by_key(|account| !account.default);
        accounts
    }

    /// What Settings lists: the default GitHub account as the one GitHub row, then the rest. A
    /// coding agent's token is set in Agent accounts and is not listed.
    pub fn credentials(&self) -> Vec<Credential> {
        let mut github_seen = false;
        let mut out = Vec::new();
        for saved in self.saved() {
            let shown = last4(saved.secret.expose_secret());
            let (kind, account, env_var) = match &saved.slot {
                Slot::GitHub => {
                    if std::mem::replace(&mut github_seen, true) {
                        continue;
                    }
                    let login = saved.github.map(|login| login.login);
                    (CredentialKind::GitHub {}, login, GITHUB_ENV_VAR.to_owned())
                }
                Slot::Forgejo(url) => (
                    CredentialKind::Forgejo {
                        url: url.as_str().to_owned(),
                    },
                    None,
                    String::new(),
                ),
                Slot::ApiKey(provider) => (
                    CredentialKind::ApiKey {
                        provider: provider.slug().to_owned(),
                    },
                    None,
                    provider.env_var().to_owned(),
                ),
                Slot::AgentToken(_) => continue,
            };
            out.push(Credential {
                id: saved.id,
                kind,
                last4: shown,
                account,
                env_var,
                to_machines: true,
                created_at: time(saved.saved_ms),
                unreadable: false,
            });
        }
        out
    }

    /// Checks `secret` with its host and saves it. A GitHub token is saved as the account it signs
    /// in as. A key only the server uses (`to_machines` off) is [`FleetError::NeedsServer`].
    pub async fn save_credential(
        &self,
        kind: &CredentialKind,
        secret: &SecretText,
        to_machines: bool,
    ) -> Result<Credential> {
        let slot = Slot::from_wire(kind).map_err(|e| FleetError::refused(e.to_string()))?;
        if !to_machines && !slot.always_to_machines() {
            return Err(FleetError::NeedsServer);
        }
        let secret = trimmed(secret)?;
        if slot == Slot::GitHub {
            self.save_github(&secret, true).await?;
            return self
                .credentials()
                .into_iter()
                .find(|c| matches!(c.kind, CredentialKind::GitHub {}))
                .ok_or(FleetError::NotFound);
        }
        let account = self.check(&slot, &secret).await?;
        let _write = WRITES.lock().await;
        let saved_ms = now_ms();
        let (id, value) = match &slot {
            Slot::Forgejo(url) => (
                make_id(FORGEJO, saved_ms, &url.host()),
                Value::Forgejo(ForgejoLogin {
                    url: url.as_str().to_owned(),
                    token: SecretText::new(secret.expose_secret()),
                }),
            ),
            Slot::ApiKey(provider) => (
                make_id(API, saved_ms, provider.slug()),
                env(provider.env_var(), &secret),
            ),
            Slot::AgentToken(agent) => (
                make_id(AGENT, saved_ms, agent.slug()),
                env(agent.env_var(), &secret),
            ),
            Slot::GitHub => return Err(FleetError::NotFound),
        };
        // One of each: a new token for the same slot replaces the old one.
        for old in self.saved().into_iter().filter(|old| old.slot == slot) {
            self.drop_keys(&old.id).await?;
        }
        crate::records::write(&self.mesh, key(&id), Some(&value)).await?;
        Ok(Credential {
            id,
            kind: kind.clone(),
            last4: last4(secret.expose_secret()),
            account,
            env_var: slot.env_var().unwrap_or_default().to_owned(),
            to_machines: true,
            created_at: time(saved_ms),
            unreadable: false,
        })
    }

    /// Checks a GitHub token, reads its orgs and saves it as its account. The first account is
    /// the default, and so is one saved with `replace_default`.
    pub async fn save_github(
        &self,
        secret: &SecretString,
        replace_default: bool,
    ) -> Result<GitHubAccount> {
        let checker = self.checker()?;
        let login = match self.check(&Slot::GitHub, secret).await? {
            Some(login) => login,
            None => {
                return Err(FleetError::refused(
                    "GitHub did not say who the token signs in as. Try again.",
                ));
            }
        };
        let orgs = checker.github_orgs(secret).await;
        let _write = WRITES.lock().await;
        let mut accounts = self.saved();
        accounts.retain(|saved| saved.github.is_some());
        let first = accounts.is_empty();
        let id = make_id(GITHUB, now_ms(), &login);
        let default = first || replace_default;
        // The same login saved again replaces its account.
        let mut kept: Vec<(String, u64, SecretString, GitHubLogin)> = Vec::new();
        for saved in accounts {
            let Some(mut account) = saved.github else {
                continue;
            };
            if account.login.eq_ignore_ascii_case(&login) {
                continue;
            }
            account.default &= !default;
            kept.push((saved.id, saved.saved_ms, saved.secret, account));
        }
        kept.push((
            id.clone(),
            now_ms(),
            secret.clone(),
            GitHubLogin {
                login,
                orgs,
                default,
            },
        ));
        self.write_github(kept).await?;
        self.github_accounts()
            .into_iter()
            .find(|account| account.id == id)
            .ok_or(FleetError::NotFound)
    }

    /// Makes the account `id` the one machines get as `GH_TOKEN`.
    pub async fn set_default_github(&self, id: &str) -> Result<()> {
        let _write = WRITES.lock().await;
        let mut found = false;
        let mut accounts = Vec::new();
        for saved in self.saved() {
            let Some(mut account) = saved.github else {
                continue;
            };
            account.default = saved.id == id;
            found |= account.default;
            accounts.push((saved.id, saved.saved_ms, saved.secret, account));
        }
        if !found {
            return Err(FleetError::NotFound);
        }
        self.write_github(accounts).await
    }

    /// Writes the keys of every GitHub account: its own, with `GH_TOKEN` for the default and an
    /// owner variable for each other, and one owner key per owner it is the one to serve. Keys of
    /// accounts no longer listed go.
    async fn write_github(
        &self,
        accounts: Vec<(String, u64, SecretString, GitHubLogin)>,
    ) -> Result<()> {
        let opened: Vec<Opened> = accounts
            .iter()
            .map(|(_, _, secret, login)| Opened {
                slot: Slot::GitHub,
                secret: secret.clone(),
                github: Some(login.clone()),
            })
            .collect();
        let mut wanted: BTreeMap<String, Value> = BTreeMap::new();
        for (id, _, secret, login) in &accounts {
            let name = match login.default {
                true => Some(GITHUB_ENV_VAR.to_owned()),
                false => github_owner_var(&login.login),
            };
            if let Some(name) = name {
                wanted.insert(id.clone(), env(&name, secret));
            }
        }
        let mut owners: Vec<&str> = Vec::new();
        for (_, _, _, login) in &accounts {
            for owner in std::iter::once(&login.login).chain(&login.orgs) {
                if !owners.iter().any(|seen| seen.eq_ignore_ascii_case(owner)) {
                    owners.push(owner);
                }
            }
        }
        for owner in owners {
            let Some(picked) = github_for(&opened, owner) else {
                continue;
            };
            let Some(at) = opened.iter().position(|o| std::ptr::eq(o, picked)) else {
                continue;
            };
            let (id, _, secret, login) = &accounts[at];
            // The account's own login is its main key; an org it serves is an owner key.
            if login.login.eq_ignore_ascii_case(owner) {
                continue;
            }
            if let Some(name) = github_owner_var(owner) {
                wanted.insert(format!("{id}:{owner}"), env(&name, secret));
            }
        }
        // Gone: every GitHub key that is not wanted now.
        for (stale, _) in self.keys_of(GITHUB) {
            if !wanted.contains_key(&stale) {
                crate::records::write::<Value>(&self.mesh, key(&stale), None).await?;
            }
        }
        for (id, value) in wanted {
            let current = crate::records::read::<Value>(&self.mesh, &key(&id));
            if current.as_ref() != Some(&value) {
                crate::records::write(&self.mesh, key(&id), Some(&value)).await?;
            }
        }
        Ok(())
    }

    /// Every key of `kind`, with its value.
    fn keys_of(&self, kind: &str) -> Vec<(String, Value)> {
        crate::records::scan::<Value>(&self.mesh, &format!("{PREFIX}{kind}."))
            .into_iter()
            .filter_map(|(key, value)| match key {
                Key::Credential(id) => Some((id, value)),
                _ => None,
            })
            .collect()
    }

    /// Deletes the credential `id` and the owner keys that go with it.
    async fn drop_keys(&self, id: &str) -> Result<()> {
        let owners = format!("{id}:");
        for (key_id, _) in self.keys_of(parse_id(id).map_or("", |(kind, _, _)| kind)) {
            if key_id == id || key_id.starts_with(&owners) {
                crate::records::write::<Value>(&self.mesh, key(&key_id), None).await?;
            }
        }
        Ok(())
    }

    /// Removes a credential. Removing the default GitHub account leaves none the default, so the
    /// oldest left takes over.
    pub async fn remove_credential(&self, id: &str) -> Result<()> {
        let _write = WRITES.lock().await;
        let saved = self.saved();
        let Some(removed) = saved.iter().find(|saved| saved.id == id) else {
            return Err(FleetError::NotFound);
        };
        if removed.github.is_none() {
            return self.drop_keys(id).await;
        }
        let mut left: Vec<(String, u64, SecretString, GitHubLogin)> = saved
            .into_iter()
            .filter(|saved| saved.id != id)
            .filter_map(|saved| Some((saved.id, saved.saved_ms, saved.secret, saved.github?)))
            .collect();
        if !left.is_empty() && !left.iter().any(|(_, _, _, login)| login.default) {
            left[0].3.default = true;
        }
        self.write_github(left).await
    }

    /// Asks the host whether `secret` works and who it signs in as.
    async fn check(&self, slot: &Slot, secret: &SecretString) -> Result<Option<String>> {
        let checker = self.checker()?;
        let checked = match slot {
            Slot::GitHub => checker.github(secret).await,
            Slot::Forgejo(url) => checker.forgejo(url, secret).await,
            Slot::ApiKey(ApiProvider::OpenRouter) => checker.openrouter(secret).await,
            Slot::ApiKey(_) | Slot::AgentToken(_) => return Ok(None),
        };
        match checked {
            Checked::Valid { login } => Ok(login),
            Checked::Refused(why) => Err(FleetError::refused(why)),
            Checked::Unreachable if *slot == Slot::GitHub => Err(FleetError::refused(
                "Could not reach GitHub to check the token. Try again.",
            )),
            Checked::Unreachable => Ok(None),
        }
    }

    fn checker(&self) -> Result<Checker> {
        let checker = Checker::new(self.hosts_client()?);
        Ok(match &self.github_api {
            Some(base) => checker.github_at(base),
            None => checker,
        })
    }

    fn hosts_client(&self) -> Result<reqwest::Client> {
        use beep_projects::imports::hosts::Hosts;
        Ok(self.hosts.client()?)
    }

    /// Checks a saved Forgejo token: from this Mac, else with teacli from the machines that have
    /// it.
    pub async fn check_credential(&self, id: &str) -> Result<CredentialCheck> {
        let saved = self.saved();
        let Some(found) = saved.iter().find(|saved| saved.id == id) else {
            return Err(FleetError::NotFound);
        };
        let Slot::Forgejo(url) = &found.slot else {
            return Err(FleetError::NotFound);
        };
        let checker = self.checker()?;
        Ok(match checker.forgejo(url, &found.secret).await {
            Checked::Valid {
                login: Some(account),
            } => CredentialCheck::Valid {
                account,
                machine: None,
            },
            Checked::Valid { login: None } => CredentialCheck::Unreachable {
                message: format!("{} did not say who the token signs in as.", url.host()),
            },
            Checked::Refused(message) => CredentialCheck::Refused {
                machine: None,
                message,
            },
            Checked::Unreachable => check_from(&self.teacli_machines().await, url).await,
        })
    }

    /// The online machines that run commands and report teacli, at most [`MAX_MACHINES`].
    async fn teacli_machines(&self) -> Vec<Reach> {
        let mut out = Vec::new();
        for machine in self.machines().await {
            let Some(shell) = machine.shell.clone() else {
                continue;
            };
            let Ok(id) = machine.id.parse::<EndpointId>() else {
                continue;
            };
            let Ok(client) = self.peers.client(id).await else {
                continue;
            };
            let has_teacli = probe(&client).await.is_ok_and(|said| {
                said.env
                    .is_some_and(|env| env.tools.iter().any(|tool| tool.name == "teacli"))
            });
            if has_teacli {
                out.push(Reach {
                    name: machine.name,
                    shell,
                });
            }
            if out.len() == MAX_MACHINES {
                break;
            }
        }
        out
    }

    /// The tokens Beep keeps for the coding agents, and the machines that have them.
    pub async fn agent_accounts(&self) -> Result<AgentAccounts> {
        let token = |agent: AgentAccount| {
            self.saved()
                .into_iter()
                .find(|saved| saved.slot == Slot::AgentToken(agent))
                .map(|saved| AgentToken {
                    last4: last4(saved.secret.expose_secret()),
                    created_at: time(saved.saved_ms),
                    unreadable: false,
                })
        };
        let mut machines = Vec::new();
        for status in self.mesh.devices() {
            let id = status.device.id;
            let said = match status.online {
                true => match self.peers.client(id).await {
                    Ok(client) => probe(&client).await.ok(),
                    Err(_) => None,
                },
                false => None,
            };
            let tools = said.as_ref().and_then(|said| said.env.as_ref());
            let has =
                |name: &str| tools.is_some_and(|env| env.tools.iter().any(|t| t.name == name));
            let (claude, codex) = (has("claude"), has("codex"));
            if !claude && !codex {
                continue;
            }
            machines.push(AgentMachine {
                machine_id: id.to_string(),
                machine_name: status.device.name,
                online: status.online,
                claude,
                codex,
                claude_sessions: 0,
                codex_signed_in: tools.and_then(|env| {
                    env.logins
                        .iter()
                        .find(|login| login.tool == "codex")
                        .map(|login| login.signed_in)
                }),
            });
        }
        machines.sort_by_key(|machine| machine.machine_name.to_lowercase());
        Ok(AgentAccounts {
            claude: token(AgentAccount::Claude),
            codex_key: token(AgentAccount::Codex),
            machines,
        })
    }

    /// Saves the token `agent` signs in with, for every machine.
    pub async fn save_agent_token(&self, agent: &str, secret: &SecretText) -> Result<AgentToken> {
        let agent = AgentAccount::from_str(agent)
            .map_err(|_| FleetError::refused("Beep keeps a token for Claude Code and Codex."))?;
        let secret = trimmed(secret)?;
        let _write = WRITES.lock().await;
        let saved_ms = now_ms();
        for old in self
            .saved()
            .into_iter()
            .filter(|old| old.slot == Slot::AgentToken(agent))
        {
            self.drop_keys(&old.id).await?;
        }
        let id = make_id(AGENT, saved_ms, agent.slug());
        crate::records::write(&self.mesh, key(&id), Some(&env(agent.env_var(), &secret))).await?;
        Ok(AgentToken {
            last4: last4(secret.expose_secret()),
            created_at: time(saved_ms),
            unreadable: false,
        })
    }

    pub async fn revoke_agent_token(&self, agent: &str) -> Result<()> {
        let agent = AgentAccount::from_str(agent).map_err(|_| FleetError::NotFound)?;
        let _write = WRITES.lock().await;
        let old = self
            .saved()
            .into_iter()
            .find(|old| old.slot == Slot::AgentToken(agent))
            .ok_or(FleetError::NotFound)?;
        self.drop_keys(&old.id).await
    }
}
