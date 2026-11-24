//! Lazily managed language servers for editor go-to-definition (`Pool`).

mod client;
pub(crate) mod protocol;

use self::{
    client::{Client, ERR_CONTENT_MODIFIED, ERR_SERVER_CANCELLED, RpcError},
    protocol::{Language, find_server, language_for, parse_locations},
};
use super::work_folder::WorkRoot;
use crate::{AsyncMutex, machine::provision::spawn_env};
use anyhow::{Context, Result, bail};
use beep_proto::node::agent::{LspLocation, RelPath};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{fs, time::sleep};

/// Idle servers shut down after this long without a request.
const IDLE_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// Most language servers kept running at once across workspaces.
const MAX_SERVERS: usize = 6;
/// Largest file buffer sent to a language server.
const MAX_DOCUMENT_BYTES: usize = 4 * 1024 * 1024;
/// Wait before retrying once after a transient `ContentModified` response.
const RETRY_DELAY: Duration = Duration::from_millis(250);

/// What a definition lookup produced: resolved targets and an optional user-facing note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DefinitionOutcome {
    pub(super) locations: Vec<LspLocation>,
    pub(super) note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ClientKey {
    root: PathBuf,
    language: Language,
}

/// Pool of lazily spawned language servers keyed by `(workspace_root, language)`.
#[derive(Default)]
pub(super) struct Pool {
    clients: AsyncMutex<HashMap<ClientKey, Arc<Client>>>,
}

impl Pool {
    /// Resolves the definition at 0-based `line` and UTF-16 `column` of `rel` inside `root`.
    pub(super) async fn definition(
        &self,
        root: &Path,
        rel: &RelPath,
        line: u32,
        column: u32,
        text: Option<String>,
    ) -> Result<DefinitionOutcome> {
        let Some((language, language_id)) = language_for(rel.as_ref()) else {
            return Ok(DefinitionOutcome {
                locations: Vec::new(),
                note: Some("No language server configured for this file type.".into()),
            });
        };

        let work = WorkRoot::at(root)?;
        let rel_path = PathBuf::from(rel);
        let Some(file_path) = work.find(&rel_path)? else {
            bail!("{} is not in this folder", rel);
        };
        let canonical_root = work.root().to_path_buf();

        let source = match text {
            Some(buf) => {
                if buf.len() > MAX_DOCUMENT_BYTES {
                    bail!("file is too large for language server lookup");
                }
                buf
            }
            None => {
                let bytes = fs::read(&file_path)
                    .await
                    .with_context(|| format!("reading {}", rel))?;
                if bytes.len() > MAX_DOCUMENT_BYTES {
                    bail!("file is too large for language server lookup");
                }
                String::from_utf8(bytes).context("file is not UTF-8 text")?
            }
        };

        let key = ClientKey {
            root: canonical_root.clone(),
            language,
        };
        let Some(client) = self.client_for(&key).await? else {
            return Ok(DefinitionOutcome {
                locations: Vec::new(),
                note: Some(format!(
                    "No {} language server found (install {}).",
                    language.label(),
                    language.install_hint()
                )),
            });
        };

        if let Err(err) = client.sync_document(&file_path, language_id, &source).await {
            self.evict_dead(&key, &client).await;
            return Err(err);
        }

        let response = match client.definition(&file_path, line, column).await {
            Ok(value) => value,
            Err(err) if is_retryable(&err) => {
                sleep(RETRY_DELAY).await;
                match client.definition(&file_path, line, column).await {
                    Ok(value) => value,
                    Err(retry_err) => {
                        self.evict_dead(&key, &client).await;
                        return Err(retry_err);
                    }
                }
            }
            Err(err) => {
                self.evict_dead(&key, &client).await;
                return Err(err);
            }
        };

        let locations = parse_locations(&response, client.root());
        Ok(DefinitionOutcome {
            locations,
            note: None,
        })
    }

    async fn client_for(&self, key: &ClientKey) -> Result<Option<Arc<Client>>> {
        let mut stale = Vec::new();
        let existing = {
            let mut clients = self.clients.lock().await;
            clients.retain(|existing_key, client| {
                let keep = client.is_alive()
                    && (existing_key == key || client.idle_for() < IDLE_TIMEOUT);
                if !keep {
                    stale.push(Arc::clone(client));
                }
                keep
            });
            clients.get(key).cloned()
        };

        shutdown_all(stale);

        if let Some(client) = existing {
            client.touch();
            return Ok(Some(client));
        }

        let search_path = spawn_env::search_path();
        let home = dirs::home_dir();
        let Some((binary, args)) = find_server(
            key.language,
            &key.root,
            search_path.as_deref(),
            home.as_deref(),
        ) else {
            return Ok(None);
        };

        let spawned = Client::spawn(&key.root, &binary, args).await?;
        let mut evicted = Vec::new();

        {
            let mut clients = self.clients.lock().await;

            // Another concurrent request may have spawned one first.
            if let Some(winner) = clients.get(key).filter(|c| c.is_alive()).cloned() {
                evicted.push(spawned);
                drop(clients);
                shutdown_all(evicted);
                winner.touch();
                return Ok(Some(winner));
            }

            while clients.len() >= MAX_SERVERS {
                let oldest = clients
                    .iter()
                    .max_by_key(|(_, client)| client.idle_for())
                    .map(|(oldest_key, _)| oldest_key.clone());

                if let Some(oldest_key) = oldest
                    && let Some(removed) = clients.remove(&oldest_key)
                {
                    evicted.push(removed);
                } else {
                    break;
                }
            }

            clients.insert(key.clone(), Arc::clone(&spawned));
        }

        shutdown_all(evicted);
        Ok(Some(spawned))
    }

    async fn evict_dead(&self, key: &ClientKey, client: &Arc<Client>) {
        if client.is_alive() {
            return;
        }

        let removed = {
            let mut clients = self.clients.lock().await;
            match clients.get(key) {
                Some(current) if Arc::ptr_eq(current, client) => clients.remove(key),
                _ => None,
            }
        };

        if let Some(dead) = removed {
            shutdown_all(vec![dead]);
        }
    }
}

fn is_retryable(err: &anyhow::Error) -> bool {
    err.downcast_ref::<RpcError>().is_some_and(|rpc| {
        matches!(rpc.code, ERR_CONTENT_MODIFIED | ERR_SERVER_CANCELLED)
    })
}

fn shutdown_all(clients: Vec<Arc<Client>>) {
    for client in clients {
        tokio::spawn(async move {
            client.shutdown().await;
        });
    }
}

#[cfg(test)]
#[path = "../../tests/agents_lsp.rs"]
mod tests;
