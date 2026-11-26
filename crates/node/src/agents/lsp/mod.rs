//! Lazily managed language servers for editor go-to-definition (`Pool`).

mod client;

use self::client::{Client, ERR_CONTENT_MODIFIED, ERR_SERVER_CANCELLED, RpcError};
use super::work_folder::WorkRoot;
use crate::{
    AsyncMutex,
    machine::{env::locate, spawn_env},
};
use anyhow::{Context, Result, bail};
use beep_proto::node::agent::{LspDefinitions, RelPath};
use std::{
    collections::HashMap,
    ffi::OsStr,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{fs, task::spawn_blocking, time::sleep};

/// Idle servers shut down after this long without a request.
const IDLE_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// Most language servers kept running at once across workspaces.
const MAX_SERVERS: usize = 6;
/// Largest file buffer sent to a language server.
const MAX_DOCUMENT_BYTES: usize = 4 * 1024 * 1024;
/// Wait before retrying once after a transient `ContentModified` response.
const RETRY_DELAY: Duration = Duration::from_millis(250);

/// A language family that shares one language server process per workspace root.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Language {
    Rust,
    TypeScript,
    Python,
    Go,
    CCpp,
    Swift,
    Zig,
    Lua,
    Shell,
    Ruby,
    Java,
    Kotlin,
}

struct LangSpec {
    label: &'static str,
    install_hint: &'static str,
    local_dirs: &'static [&'static str],
    servers: &'static [(&'static str, &'static [&'static str])],
}

const NODE_BINS: &[&str] = &["node_modules/.bin"];
const PYTHON_BINS: &[&str] = &[".venv/bin", "venv/bin", ".venv/Scripts", "venv/Scripts"];

impl Language {
    fn spec(self) -> &'static LangSpec {
        match self {
            Self::Rust => &LangSpec {
                label: "Rust",
                install_hint: "rust-analyzer",
                local_dirs: &[],
                servers: &[("rust-analyzer", &[])],
            },
            Self::TypeScript => &LangSpec {
                label: "TypeScript",
                install_hint: "vtsls or typescript-language-server",
                local_dirs: NODE_BINS,
                servers: &[
                    ("vtsls", &["--stdio"]),
                    ("typescript-language-server", &["--stdio"]),
                ],
            },
            Self::Python => &LangSpec {
                label: "Python",
                install_hint: "basedpyright or pyright",
                local_dirs: PYTHON_BINS,
                servers: &[
                    ("basedpyright-langserver", &["--stdio"]),
                    ("pyright-langserver", &["--stdio"]),
                    ("pylsp", &[]),
                    ("ty", &["server"]),
                ],
            },
            Self::Go => &LangSpec {
                label: "Go",
                install_hint: "gopls",
                local_dirs: &[],
                servers: &[("gopls", &[])],
            },
            Self::CCpp => &LangSpec {
                label: "C/C++",
                install_hint: "clangd",
                local_dirs: &[],
                servers: &[("clangd", &[])],
            },
            Self::Swift => &LangSpec {
                label: "Swift",
                install_hint: "sourcekit-lsp",
                local_dirs: &[],
                servers: &[("sourcekit-lsp", &[])],
            },
            Self::Zig => &LangSpec {
                label: "Zig",
                install_hint: "zls",
                local_dirs: &[],
                servers: &[("zls", &[])],
            },
            Self::Lua => &LangSpec {
                label: "Lua",
                install_hint: "lua-language-server",
                local_dirs: &[],
                servers: &[("lua-language-server", &[])],
            },
            Self::Shell => &LangSpec {
                label: "Shell",
                install_hint: "bash-language-server",
                local_dirs: NODE_BINS,
                servers: &[("bash-language-server", &["start"])],
            },
            Self::Ruby => &LangSpec {
                label: "Ruby",
                install_hint: "ruby-lsp",
                local_dirs: &[],
                servers: &[("ruby-lsp", &[]), ("solargraph", &["stdio"])],
            },
            Self::Java => &LangSpec {
                label: "Java",
                install_hint: "jdtls",
                local_dirs: &[],
                servers: &[("jdtls", &[])],
            },
            Self::Kotlin => &LangSpec {
                label: "Kotlin",
                install_hint: "kotlin-language-server",
                local_dirs: &[],
                servers: &[("kotlin-language-server", &[])],
            },
        }
    }
}

/// Maps a file path's extension to its server family and LSP `languageId`.
pub(super) fn language_for(path: &str) -> Option<(Language, &'static str)> {
    let ext = Path::new(path).extension()?.to_str()?.to_ascii_lowercase();

    Some(match ext.as_str() {
        "rs" => (Language::Rust, "rust"),
        "ts" | "mts" | "cts" => (Language::TypeScript, "typescript"),
        "tsx" => (Language::TypeScript, "typescriptreact"),
        "js" | "mjs" | "cjs" => (Language::TypeScript, "javascript"),
        "jsx" => (Language::TypeScript, "javascriptreact"),
        "py" | "pyi" => (Language::Python, "python"),
        "go" => (Language::Go, "go"),
        "c" | "h" => (Language::CCpp, "c"),
        "cc" | "cpp" | "cxx" | "hpp" | "hh" | "hxx" => (Language::CCpp, "cpp"),
        "swift" => (Language::Swift, "swift"),
        "zig" => (Language::Zig, "zig"),
        "lua" => (Language::Lua, "lua"),
        "sh" | "bash" | "zsh" => (Language::Shell, "shellscript"),
        "rb" => (Language::Ruby, "ruby"),
        "java" => (Language::Java, "java"),
        "kt" | "kts" => (Language::Kotlin, "kotlin"),
        _ => return None,
    })
}

/// Finds the first installed language server for `language`, preferring workspace-local
/// binaries, then `search_path`, then common user install folders under `home`.
pub(super) fn find_server(
    language: Language,
    root: &Path,
    search_path: Option<&OsStr>,
    home: Option<&Path>,
) -> Option<(PathBuf, &'static [&'static str])> {
    let spec = language.spec();
    let exts: &[&str] = if cfg!(windows) {
        &[".exe", ".cmd", ".bat"]
    } else {
        &[""]
    };

    for &(program, args) in spec.servers {
        for dir in spec.local_dirs {
            for ext in exts {
                let candidate = root.join(dir).join(format!("{program}{ext}"));
                if candidate.is_file() {
                    return Some((candidate, args));
                }
            }
        }

        if let Ok(found) = which::which_in(program, search_path, root) {
            return Some((found, args));
        }

        if let Some(found) = home.and_then(|home| locate::fallback(program, home)) {
            return Some((found, args));
        }
    }

    None
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
    ) -> Result<LspDefinitions> {
        let Some((language, language_id)) = language_for(rel.as_ref()) else {
            return Ok(LspDefinitions {
                locations: Vec::new(),
                note: Some("No language server configured for this file type.".into()),
            });
        };

        let root_buf = root.to_path_buf();
        let rel_path = PathBuf::from(rel);
        let rel_display = rel.to_string();
        let file_path = spawn_blocking(move || {
            let work = WorkRoot::at(&root_buf)?;
            work.find(&rel_path)?
                .ok_or_else(|| anyhow::anyhow!("{rel_display} is not in this folder"))
        })
        .await
        .context("resolving file inside workspace")??;

        let source = read_source(&file_path, rel, text).await?;
        let key = ClientKey {
            root: root.to_path_buf(),
            language,
        };
        let Some(client) = self.client_for(&key).await? else {
            let spec = language.spec();
            return Ok(LspDefinitions {
                locations: Vec::new(),
                note: Some(format!(
                    "No {} language server found (install {}).",
                    spec.label, spec.install_hint
                )),
            });
        };

        let result = query_definition(&client, &file_path, language_id, &source, line, column).await;

        match result {
            Ok(locations) => Ok(LspDefinitions {
                locations,
                note: None,
            }),
            Err(err) => {
                self.evict_dead(&key, &client).await;
                Err(err)
            }
        }
    }

    /// Shuts down every running language server, for node shutdown.
    pub(super) async fn shutdown(&self) {
        let clients: Vec<Arc<Client>> = self.clients.lock().await.drain().map(|(_, c)| c).collect();
        futures::future::join_all(clients.iter().map(|client| client.shutdown())).await;
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
        shutdown_background(stale);

        if let Some(client) = existing {
            client.touch();
            return Ok(Some(client));
        }

        let language = key.language;
        let root = key.root.clone();
        let search_path = spawn_env::search_path();
        let home = dirs::home_dir();
        let found = spawn_blocking(move || {
            find_server(language, &root, search_path.as_deref(), home.as_deref())
        })
        .await
        .context("looking up language server")?;

        let Some((binary, args)) = found else {
            return Ok(None);
        };

        let spawned = Client::spawn(&key.root, &binary, args).await?;
        let mut evicted = Vec::new();

        {
            let mut clients = self.clients.lock().await;

            if let Some(winner) = clients.get(key).filter(|c| c.is_alive()).cloned() {
                drop(clients);
                shutdown_background(vec![spawned]);
                winner.touch();
                return Ok(Some(winner));
            }

            while clients.len() >= MAX_SERVERS {
                let Some(oldest_key) = clients
                    .iter()
                    .max_by_key(|(_, client)| client.idle_for())
                    .map(|(k, _)| k.clone())
                else {
                    break;
                };
                if let Some(removed) = clients.remove(&oldest_key) {
                    evicted.push(removed);
                }
            }

            clients.insert(key.clone(), Arc::clone(&spawned));
        }

        shutdown_background(evicted);
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
            shutdown_background(vec![dead]);
        }
    }
}

async fn read_source(file_path: &Path, rel: &RelPath, text: Option<String>) -> Result<String> {
    if let Some(buf) = text {
        if buf.len() > MAX_DOCUMENT_BYTES {
            bail!("file is too large for language server lookup");
        }
        return Ok(buf);
    }

    let meta = fs::metadata(file_path)
        .await
        .with_context(|| format!("reading {rel}"))?;
    if meta.len() > MAX_DOCUMENT_BYTES as u64 {
        bail!("file is too large for language server lookup");
    }

    let bytes = fs::read(file_path)
        .await
        .with_context(|| format!("reading {rel}"))?;
    if bytes.len() > MAX_DOCUMENT_BYTES {
        bail!("file is too large for language server lookup");
    }

    String::from_utf8(bytes).context("file is not UTF-8 text")
}

async fn query_definition(
    client: &Client,
    file_path: &Path,
    language_id: &str,
    source: &str,
    line: u32,
    column: u32,
) -> Result<Vec<beep_proto::node::agent::LspLocation>> {
    client.sync_document(file_path, language_id, source).await?;

    match client.definition(file_path, line, column).await {
        Ok(locations) => Ok(locations),
        Err(err) if is_retryable(&err) => {
            sleep(RETRY_DELAY).await;
            client.definition(file_path, line, column).await
        }
        Err(err) => Err(err),
    }
}

fn is_retryable(err: &anyhow::Error) -> bool {
    err.downcast_ref::<RpcError>().is_some_and(|rpc| {
        matches!(rpc.code, ERR_CONTENT_MODIFIED | ERR_SERVER_CANCELLED)
    })
}

fn shutdown_background(clients: Vec<Arc<Client>>) {
    for client in clients {
        tokio::spawn(async move {
            client.shutdown().await;
        });
    }
}

#[cfg(test)]
#[path = "../../tests/agents_lsp.rs"]
mod tests;
