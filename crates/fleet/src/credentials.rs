//! The user's credentials, kept in the mesh as `credential/<id>`: the values the nodes read
//! (`Credential::Env` and `Credential::Forgejo`), with everything else the desktop shows derived
//! from the key and the value. An id is `<kind>.<saved_ms>.<name>`: `gh.<ms>.<login>` for a
//! GitHub account, whose `~<owner>` siblings carry the same token for each owner it serves,
//! `forgejo.<ms>.<host>`, `agent.<ms>.<agent>` and `key.<ms>.<provider>` for an API key. Released
//! desktops delete `api.*` keys, so API keys live under `key.*`. A coding agent's named account is
//! `agent.<ms>.<agent>.<name>`, a `Credential::Agent` that released nodes and desktops skip.
//!
//! A secret never reaches a log: [`SecretText`] and [`SecretString`] print nothing.

use crate::{
    desk::Desk,
    error::{FleetError, Result},
    peer::probe,
    records::{UNMOVED_CREDENTIAL, Unmoved, unmoved_credential},
    records::{WRITES, now_ms, time},
};
use beep_mesh::{DeviceStatus, Key};
use beep_projects::{
    credential_check::{
        Checked, Checker,
        machines::{MAX_MACHINES, Reach, check_from},
    },
    credentials::{GitHubLogin, Opened, github_for, last4},
    github_owner_var,
};
use beep_proto::{
    AccountName, AgentAccount, AgentAccounts, AgentMachine, AgentToken, ApiKey, Credential,
    CredentialCheck, ForgejoUrl, GITHUB_ENV_VAR, GitHubAccount, NamedAgentToken, SecretText,
    Service,
    node::{AgentLogin, EnvReport, EnvSecret, ForgejoLogin, mesh_state::Credential as Value},
};
use futures::{StreamExt, stream};
use iroh::EndpointId;
use secrecy::{ExposeSecret, SecretString};
use std::{collections::BTreeMap, str::FromStr, time::Duration};
use tokio::time::timeout;

/// Machines asked about their agents at once.
const ACCOUNTS_AT_ONCE: usize = 4;
/// How long one machine has to answer, so an unreachable one does not hold the card.
const ACCOUNTS_PER_MACHINE: Duration = Duration::from_secs(8);
/// The longest secret taken.
const MAX_SECRET: usize = 4096;
const PREFIX: &str = "credential/";

const GITHUB: &str = "gh";
const FORGEJO: &str = "forgejo";
/// API keys the machines get as env vars, for flsh's tools.
const API_KEY: &str = "key";
const AGENT: &str = "agent";

/// A saved credential, read back from its keys.
struct Saved {
    /// The id of its main key.
    id: String,
    saved_ms: u64,
    service: Service,
    secret: SecretString,
    /// For a GitHub account.
    github: Option<GitHubLogin>,
}

impl Saved {
    fn opened(&self) -> Opened {
        Opened {
            service: self.service.clone(),
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
        let mut found = self.credential_keys(PREFIX);
        found.sort_by(|a, b| a.0.cmp(&b.0));
        let mut out: Vec<Saved> = Vec::new();
        let mut orgs: BTreeMap<String, Vec<String>> = BTreeMap::new();

        for (id, value) in &found {
            if let Some((main, owner)) = id.split_once('~') {
                orgs.entry(main.to_owned())
                    .or_default()
                    .push(owner.to_owned());
                continue;
            }

            let Some((kind, saved_ms, name)) = parse_id(id) else {
                continue;
            };
            let (service, secret, github) = match (kind, value) {
                (GITHUB, Value::Env(secret)) => (
                    Service::GitHub,
                    text(&secret.value),
                    Some(GitHubLogin {
                        login: name.to_owned(),
                        orgs: Vec::new(),
                        default: secret.name == GITHUB_ENV_VAR,
                    }),
                ),
                (FORGEJO, Value::Forgejo(login)) => {
                    let Ok(url) = login.url.parse::<ForgejoUrl>() else {
                        continue;
                    };
                    (Service::Forgejo { url }, text(&login.token), None)
                }
                (API_KEY, Value::Env(secret)) => {
                    let Ok(provider) = ApiKey::from_str(name) else {
                        continue;
                    };
                    (Service::ApiKey { provider }, text(&secret.value), None)
                }
                (AGENT, Value::Env(secret)) => {
                    let Ok(agent) = AgentAccount::from_str(name) else {
                        continue;
                    };
                    (Service::Agent { agent }, text(&secret.value), None)
                }
                _ => continue,
            };
            out.push(Saved {
                id: id.clone(),
                saved_ms,
                service,
                secret,
                github,
            });
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

        out.sort_by_key(|saved| (saved.service != Service::GitHub, saved.saved_ms));
        out
    }

    /// The credentials as git hosts and clones use them.
    pub(crate) fn opened(&self) -> Vec<Opened> {
        self.saved().iter().map(Saved::opened).collect()
    }

    /// The saved secret for `provider`, for app-side speech-to-text without sending the key to a
    /// webview.
    pub fn api_key_secret(&self, provider: ApiKey) -> Option<SecretString> {
        self.saved()
            .into_iter()
            .find(|saved| saved.service == Service::ApiKey { provider })
            .map(|saved| saved.secret)
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
            let account = match &saved.service {
                Service::GitHub if std::mem::replace(&mut github_seen, true) => continue,
                Service::GitHub => saved.github.map(|login| login.login),
                Service::Forgejo { .. } | Service::ApiKey { .. } => None,
                Service::Agent { .. } => continue,
            };
            out.push(Credential {
                id: saved.id,
                env_var: saved.service.env_var().unwrap_or_default().to_owned(),
                service: saved.service,
                last4: shown,
                account,
                to_machines: true,
                created_at: time(saved.saved_ms),
                unreadable: false,
            });
        }

        out.extend(self.unmoved(&out));
        out
    }

    /// Credentials an account move found on its server with no value to bring: shown as ones to
    /// enter again, until one is saved for the same service.
    fn unmoved(&self, listed: &[Credential]) -> Vec<Credential> {
        let saved = self.saved();
        let held: Vec<&Service> = saved.iter().map(|saved| &saved.service).collect();
        let logins: Vec<&str> = saved
            .iter()
            .filter_map(|saved| Some(saved.github.as_ref()?.login.as_str()))
            .collect();
        let mut kept: Vec<Credential> = self
            .unmoved_credentials()
            .into_iter()
            .map(|(_, credential)| credential)
            .filter(|credential| {
                if credential.service == Service::GitHub {
                    // One record per account: it waits for its own token.
                    return !credential
                        .account
                        .as_deref()
                        .map_or(!logins.is_empty(), |login| {
                            logins.iter().any(|saved| saved.eq_ignore_ascii_case(login))
                        });
                }

                !held.contains(&&credential.service)
            })
            .filter(|credential| !listed.iter().any(|row| row.id == credential.id))
            .collect();
        kept.sort_by(|a, b| a.id.cmp(&b.id));
        kept
    }

    /// The records of credentials the account move left to enter again.
    fn unmoved_credentials(&self) -> Vec<(Key, Credential)> {
        crate::records::scan::<Unmoved>(&self.mesh, UNMOVED_CREDENTIAL)
            .into_iter()
            .filter_map(|(key, record)| match record {
                Unmoved::Credential(credential) => Some((key, credential)),
                Unmoved::HarnessEnv { .. } => None,
            })
            .collect()
    }

    /// Drops the record of the credential just saved for `service`, so nodes may take the mesh's
    /// value. GitHub has several accounts, so only the record of `login` goes, or one that does
    /// not say whose it is; the others wait for their own token.
    async fn settle_unmoved(&self, service: &Service, login: Option<&str>) -> Result<()> {
        for (key, credential) in self.unmoved_credentials() {
            let same = credential.service == *service;
            let whose = match (login, credential.account.as_deref()) {
                (Some(saved), Some(record)) => saved.eq_ignore_ascii_case(record),
                _ => true,
            };

            if same && whose {
                self.mesh.set(key, None).await?;
            }
        }

        Ok(())
    }

    /// Checks `secret` with its host and saves it. A GitHub token is saved as the account it signs
    /// in as. An API key is saved as is: its provider is not asked.
    pub async fn save_credential(
        &self,
        service: &Service,
        secret: &SecretText,
    ) -> Result<Credential> {
        let secret = trimmed(secret)?;

        if *service == Service::GitHub {
            self.save_github(&secret).await?;
            return self
                .credentials()
                .into_iter()
                .find(|c| c.service == Service::GitHub)
                .ok_or(FleetError::NotFound);
        }

        let account = self.check(service, &secret).await?;
        let _write = WRITES.lock().await;
        let saved_ms = now_ms();
        let (id, value) = match service {
            Service::Forgejo { url } => (
                make_id(FORGEJO, saved_ms, &url.host()),
                Value::Forgejo(ForgejoLogin {
                    url: url.as_ref().to_owned(),
                    token: SecretText::new(secret.expose_secret()),
                }),
            ),
            Service::ApiKey { provider } => (
                make_id(API_KEY, saved_ms, &provider.to_string()),
                env(provider.env_var(), &secret),
            ),
            Service::GitHub | Service::Agent { .. } => return Err(FleetError::NotFound),
        };
        // One of each: a new token for the same service replaces the old one.
        for old in self
            .saved()
            .into_iter()
            .filter(|old| old.service == *service)
        {
            self.drop_keys(&old.id).await?;
        }

        crate::records::write(&self.mesh, key(&id), Some(&value)).await?;
        self.settle_unmoved(service, None).await?;
        Ok(Credential {
            id,
            service: service.clone(),
            last4: last4(secret.expose_secret()),
            account,
            env_var: service.env_var().unwrap_or_default().to_owned(),
            to_machines: true,
            created_at: time(saved_ms),
            unreadable: false,
        })
    }

    /// Checks a GitHub token, reads its orgs and saves it as the default account.
    async fn save_github(&self, secret: &SecretString) -> Result<()> {
        let checker = self.checker();
        let login = match self.check(&Service::GitHub, secret).await? {
            Some(login) => login,
            None => {
                return Err(FleetError::refused(
                    "GitHub did not say who the token signs in as. Try again.",
                ));
            }
        };
        let orgs = checker.github_orgs(secret).await;
        let _write = WRITES.lock().await;
        let id = make_id(GITHUB, now_ms(), &login);
        // The same login saved again replaces its account.
        let mut kept: Vec<(String, SecretString, GitHubLogin)> = Vec::new();

        for saved in self.saved() {
            let Some(mut account) = saved.github else {
                continue;
            };

            if account.login.eq_ignore_ascii_case(&login) {
                continue;
            }

            account.default = false;
            kept.push((saved.id, saved.secret, account));
        }

        let saved_login = login.clone();
        kept.push((
            id.clone(),
            secret.clone(),
            GitHubLogin {
                login,
                orgs,
                default: true,
            },
        ));
        self.write_github(kept).await?;
        self.settle_unmoved(&Service::GitHub, Some(&saved_login))
            .await?;
        self.github_accounts()
            .into_iter()
            .find(|a| a.id == id)
            .ok_or(FleetError::NotFound)?;

        Ok(())
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
            accounts.push((saved.id, saved.secret, account));
        }

        if !found {
            return Err(FleetError::NotFound);
        }

        self.write_github(accounts).await
    }

    /// Writes the keys of every GitHub account: its own, with `GH_TOKEN` for the default and an
    /// owner variable for each other, and one owner key per owner it is the one to serve. Keys of
    /// accounts no longer listed go.
    async fn write_github(&self, accounts: Vec<(String, SecretString, GitHubLogin)>) -> Result<()> {
        let opened: Vec<Opened> = accounts
            .iter()
            .map(|(_, secret, login)| Opened {
                service: Service::GitHub,
                secret: secret.clone(),
                github: Some(login.clone()),
            })
            .collect();
        let mut wanted: BTreeMap<String, Value> = BTreeMap::new();

        for (id, secret, login) in &accounts {
            let name = match login.default {
                true => Some(GITHUB_ENV_VAR.to_owned()),
                false => github_owner_var(&login.login),
            };
            if let Some(name) = name {
                wanted.insert(id.clone(), env(&name, secret));
            }
        }

        let mut owners: Vec<&str> = Vec::new();

        for (_, _, login) in &accounts {
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
            let (id, secret, login) = &accounts[at];
            // The account's own login is its main key; an org it serves is an owner key.
            if login.login.eq_ignore_ascii_case(owner) {
                continue;
            }

            if let Some(name) = github_owner_var(owner) {
                wanted.insert(format!("{id}~{owner}"), env(&name, secret));
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
        self.credential_keys(&format!("{PREFIX}{kind}."))
    }

    /// The credential keys under `prefix`, by id.
    fn credential_keys(&self, prefix: &str) -> Vec<(String, Value)> {
        crate::records::scan::<Value>(&self.mesh, prefix)
            .into_iter()
            .filter_map(|(key, value)| match key {
                Key::Credential(id) => Some((id, value)),
                _ => None,
            })
            .collect()
    }

    /// Deletes the credential `id` and the owner keys that go with it.
    async fn drop_keys(&self, id: &str) -> Result<()> {
        let owners = format!("{id}~");

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
            // One an account move left to enter again.
            let record = unmoved_credential(id);

            if self.mesh.get(&record).is_none() {
                return Err(FleetError::NotFound);
            }

            self.mesh.set(record, None).await?;
            return Ok(());
        };

        if removed.github.is_none() {
            return self.drop_keys(id).await;
        }

        let mut left: Vec<(String, SecretString, GitHubLogin)> = saved
            .into_iter()
            .filter(|saved| saved.id != id)
            .filter_map(|saved| Some((saved.id, saved.secret, saved.github?)))
            .collect();
        if !left.is_empty() && !left.iter().any(|(_, _, login)| login.default) {
            left[0].2.default = true;
        }
        self.write_github(left).await
    }

    /// Asks the host whether `secret` works and who it signs in as.
    async fn check(&self, service: &Service, secret: &SecretString) -> Result<Option<String>> {
        let checker = self.checker();
        let checked = match service {
            Service::GitHub => checker.github(secret).await,
            Service::Forgejo { url } => checker.forgejo(url, secret).await,
            Service::ApiKey { .. } | Service::Agent { .. } => return Ok(None),
        };

        match checked {
            Checked::Valid { login } => Ok(login),
            Checked::Refused(why) => Err(FleetError::refused(why)),
            Checked::Unreachable if *service == Service::GitHub => Err(FleetError::refused(
                "Could not reach GitHub to check the token. Try again.",
            )),
            Checked::Unreachable => Ok(None),
        }
    }

    fn checker(&self) -> Checker {
        let checker = Checker::new(self.hosts.client.clone());
        match &self.github_api {
            Some(base) => checker.github_at(base),
            None => checker,
        }
    }

    /// Checks a saved Forgejo token: from this Mac, else with teacli from the machines that have
    /// it.
    pub async fn check_credential(&self, id: &str) -> Result<CredentialCheck> {
        let saved = self.saved();
        let Some(found) = saved.iter().find(|saved| saved.id == id) else {
            return Err(FleetError::NotFound);
        };
        let Service::Forgejo { url } = &found.service else {
            return Err(FleetError::NotFound);
        };
        let checker = self.checker();
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
            let Ok(client) = self.machines.client(id).await else {
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
                .find(|saved| saved.service == Service::Agent { agent })
                .map(|saved| AgentToken {
                    last4: last4(saved.secret.expose_secret()),
                    created_at: time(saved.saved_ms),
                    unreadable: false,
                })
        };
        // Owned values in the futures, so the call stays `Send` for a Tauri command.
        let mut machines: Vec<AgentMachine> = stream::iter(self.mesh.code_hosts())
            .map(|status| async move {
                let env = self.agent_env(&status).await;
                agent_machine(status, env)
            })
            .buffer_unordered(ACCOUNTS_AT_ONCE)
            .filter_map(|machine| async move { machine })
            .collect()
            .await;
        machines.sort_by_key(|machine| machine.machine_name.to_lowercase());
        Ok(AgentAccounts {
            claude: token(AgentAccount::Claude),
            codex_key: token(AgentAccount::Codex),
            machines,
            named: self.named_agent_tokens(),
        })
    }

    /// The coding agents' named accounts, by agent then name, without asking any machine.
    pub fn named_agent_tokens(&self) -> Vec<NamedAgentToken> {
        let mut named: Vec<NamedAgentToken> = self
            .named_accounts()
            .into_iter()
            .map(|(_, saved_ms, login)| NamedAgentToken {
                agent: login.agent,
                name: login.name,
                env_var: login.env.name,
                token: AgentToken {
                    last4: last4(login.env.value.expose()),
                    created_at: time(saved_ms),
                    unreadable: false,
                },
            })
            .collect();
        named.sort_by(|a, b| {
            (a.agent.as_ref(), a.name.as_ref().to_lowercase())
                .cmp(&(b.agent.as_ref(), b.name.as_ref().to_lowercase()))
        });
        named
    }

    /// The coding agents' named accounts, by id, with when each was saved.
    fn named_accounts(&self) -> Vec<(String, u64, AgentLogin)> {
        self.keys_of(AGENT)
            .into_iter()
            .filter_map(|(id, value)| {
                let (_, saved_ms, _) = parse_id(&id)?;
                match value {
                    Value::Agent(login) => Some((id, saved_ms, login)),
                    _ => None,
                }
            })
            .collect()
    }

    /// The ids of `agent`'s account `name`; more than one only after two
    /// desktops saved it at once.
    fn named_ids(&self, agent: AgentAccount, name: &AccountName) -> Vec<String> {
        self.named_accounts()
            .into_iter()
            .filter(|(_, _, login)| {
                login.agent == agent && login.name.as_ref().eq_ignore_ascii_case(name.as_ref())
            })
            .map(|(id, _, _)| id)
            .collect()
    }

    /// What `status`'s machine has: what its status stream already reported, else a bounded
    /// probe when it is online and has not reported yet.
    async fn agent_env(&self, status: &DeviceStatus) -> Option<EnvReport> {
        let id = status.device.id;

        if let Some(env) = self.machines.last_env(id) {
            return Some(env);
        }

        if !status.online {
            return None;
        }

        timeout(ACCOUNTS_PER_MACHINE, async {
            let client = self.machines.client(id).await.ok()?;
            probe(&client).await.ok()?.env
        })
        .await
        .ok()
        .flatten()
    }

    /// Saves the token `agent` signs in with, for every machine: as its
    /// account `name`, or as its default account without one.
    pub async fn save_agent_token(
        &self,
        agent: &str,
        name: Option<&str>,
        secret: &SecretText,
    ) -> Result<AgentToken> {
        let agent = AgentAccount::from_str(agent)
            .map_err(|_| FleetError::refused("Beep keeps a token for Claude Code and Codex."))?;
        let name = name.map(AccountName::from_str).transpose()?;
        let secret = trimmed(secret)?;
        let _write = WRITES.lock().await;
        let saved_ms = now_ms();
        let token = AgentToken {
            last4: last4(secret.expose_secret()),
            created_at: time(saved_ms),
            unreadable: false,
        };

        if let Some(name) = name {
            if agent == AgentAccount::Codex && secret.expose_secret().starts_with("sk-") {
                return Err(FleetError::refused(
                    "A named Codex account takes a ChatGPT personal access token. An API key goes in Codex's own row above.",
                ));
            }

            for old in self.named_ids(agent, &name) {
                self.drop_keys(&old).await?;
            }

            let id = make_id(AGENT, saved_ms, &format!("{}.{name}", agent.as_ref()));
            let login = AgentLogin {
                agent,
                env: EnvSecret {
                    name: agent.named_env_var().to_owned(),
                    value: SecretText::new(secret.expose_secret()),
                },
                name,
            };
            crate::records::write(&self.mesh, key(&id), Some(&Value::Agent(login))).await?;
            return Ok(token);
        }

        for old in self
            .saved()
            .into_iter()
            .filter(|old| old.service == Service::Agent { agent })
        {
            self.drop_keys(&old.id).await?;
        }

        let id = make_id(AGENT, saved_ms, agent.as_ref());
        crate::records::write(&self.mesh, key(&id), Some(&env(agent.env_var(), &secret))).await?;
        Ok(token)
    }

    /// Removes `agent`'s account `name`, or its default account's token
    /// without one.
    pub async fn revoke_agent_token(&self, agent: &str, name: Option<&str>) -> Result<()> {
        let agent = AgentAccount::from_str(agent).map_err(|_| FleetError::NotFound)?;
        let name = name
            .map(AccountName::from_str)
            .transpose()
            .map_err(|_| FleetError::NotFound)?;
        let _write = WRITES.lock().await;

        if let Some(name) = name {
            let ids = self.named_ids(agent, &name);

            if ids.is_empty() {
                return Err(FleetError::NotFound);
            }

            for id in ids {
                self.drop_keys(&id).await?;
            }

            return Ok(());
        }

        let old = self
            .saved()
            .into_iter()
            .find(|old| old.service == Service::Agent { agent })
            .ok_or(FleetError::NotFound)?;
        self.drop_keys(&old.id).await
    }
}

/// A machine's row, when it has Claude Code or Codex.
fn agent_machine(status: DeviceStatus, env: Option<EnvReport>) -> Option<AgentMachine> {
    let has = |name: &str| {
        env.as_ref()
            .is_some_and(|env| env.tools.iter().any(|tool| tool.name == name))
    };
    let (claude, codex) = (has("claude"), has("codex"));

    if !claude && !codex {
        return None;
    }

    Some(AgentMachine {
        machine_id: status.device.id.to_string(),
        machine_name: status.device.name,
        online: status.online,
        claude,
        codex,
        claude_sessions: 0,
        codex_signed_in: env.and_then(|env| {
            env.logins
                .iter()
                .find(|login| login.tool == "codex")
                .map(|login| login.signed_in)
        }),
    })
}
