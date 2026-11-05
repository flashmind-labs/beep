//! The MCP servers the user lists, kept in `mcp.json` next to the chats, readable by the user
//! only. A server is a command run on this Mac or an HTTP address. Environment values and tokens
//! are `SecretString`s: they are written to that file and sent to the server they are for, and
//! the window only learns that they are set.

use crate::{AssistantError, store::write_private};
use reqwest::Url;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use std::{io::ErrorKind, path::Path};
use uuid::Uuid;

const FILE: &str = "mcp.json";
/// Servers one Mac may list.
const MAX_SERVERS: usize = 16;
/// Longest server name; it ends up in every tool name.
const MAX_NAME: usize = 24;

/// A server's environment variable.
#[derive(Debug, Clone)]
pub struct EnvVar {
    pub name: String,
    pub value: SecretString,
}

/// How to reach a server.
#[derive(Debug, Clone)]
pub enum Transport {
    /// A program run on this Mac that speaks MCP on its standard input and output.
    Stdio {
        command: String,
        args: Vec<String>,
        env: Vec<EnvVar>,
    },
    /// A streamable HTTP endpoint, with an optional bearer token.
    Http {
        url: Url,
        token: Option<SecretString>,
    },
}

/// A server on the list. `name` is a short lowercase id, such as `files`.
#[derive(Debug, Clone)]
pub struct McpServer {
    pub name: String,
    pub transport: Transport,
}

/// What the window may know of a server: everything but the secrets.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ServerSummary {
    pub name: String,
    /// The command line, or the address without its credentials and query.
    pub detail: String,
    /// The names of the environment variables that are set.
    pub env: Vec<String>,
    /// A bearer token is set.
    pub token: bool,
}

/// A name as the tools carry it: lowercase letters, digits and single hyphens.
pub(crate) fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.truncate(MAX_NAME);
    out.trim_end_matches('-').to_owned()
}

impl McpServer {
    /// A server from what the user typed. The name becomes its slug.
    pub fn new(name: &str, transport: Transport) -> Result<Self, AssistantError> {
        let name = slug(name);
        if name.is_empty() {
            return Err(AssistantError::Config(
                "The server needs a name of letters or digits.".into(),
            ));
        }
        match &transport {
            Transport::Stdio { command, env, .. } => {
                if command.trim().is_empty() {
                    return Err(AssistantError::Config("The command is empty.".into()));
                }
                if let Some(bad) = env
                    .iter()
                    .find(|v| v.name.is_empty() || v.name.contains('=') || v.name.contains('\0'))
                {
                    return Err(AssistantError::Config(format!(
                        "{:?} is not a variable name.",
                        bad.name
                    )));
                }
            }
            Transport::Http { url, .. } => {
                if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
                    return Err(AssistantError::Config(
                        "The address must start with http:// or https://.".into(),
                    ));
                }
            }
        }
        Ok(Self { name, transport })
    }

    pub fn summary(&self) -> ServerSummary {
        match &self.transport {
            Transport::Stdio { command, args, env } => ServerSummary {
                name: self.name.clone(),
                detail: std::iter::once(command.as_str())
                    .chain(args.iter().map(String::as_str))
                    .collect::<Vec<_>>()
                    .join(" "),
                env: env.iter().map(|v| v.name.clone()).collect(),
                token: false,
            },
            Transport::Http { url, token } => {
                let mut shown = url.clone();
                // Neither can be put back by the window, and either may be a credential.
                shown.set_query(None);
                shown.set_fragment(None);
                shown.set_username("").ok();
                shown.set_password(None).ok();
                ServerSummary {
                    name: self.name.clone(),
                    detail: shown.to_string(),
                    env: Vec::new(),
                    token: token.is_some(),
                }
            }
        }
    }
}

// What the file holds. Plain strings, since a secret does not serialize by design.
#[derive(Serialize, Deserialize)]
struct File {
    #[serde(default)]
    servers: Vec<Stored>,
}

#[derive(Serialize, Deserialize)]
struct Stored {
    name: String,
    #[serde(flatten)]
    transport: StoredTransport,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum StoredTransport {
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: Vec<StoredEnv>,
    },
    Http {
        url: String,
        #[serde(default)]
        token: Option<String>,
    },
}

#[derive(Serialize, Deserialize)]
struct StoredEnv {
    name: String,
    value: String,
}

fn stored(server: &McpServer) -> Stored {
    let transport = match &server.transport {
        Transport::Stdio { command, args, env } => StoredTransport::Stdio {
            command: command.clone(),
            args: args.clone(),
            env: env
                .iter()
                .map(|v| StoredEnv {
                    name: v.name.clone(),
                    value: v.value.expose_secret().to_owned(),
                })
                .collect(),
        },
        Transport::Http { url, token } => StoredTransport::Http {
            url: url.to_string(),
            token: token.as_ref().map(|t| t.expose_secret().to_owned()),
        },
    };
    Stored {
        name: server.name.clone(),
        transport,
    }
}

fn restored(stored: Stored) -> Result<McpServer, AssistantError> {
    let transport = match stored.transport {
        StoredTransport::Stdio { command, args, env } => Transport::Stdio {
            command,
            args,
            env: env
                .into_iter()
                .map(|v| EnvVar {
                    name: v.name,
                    value: SecretString::from(v.value),
                })
                .collect(),
        },
        StoredTransport::Http { url, token } => Transport::Http {
            url: Url::parse(&url).map_err(|_| {
                AssistantError::Config(format!("{} has no valid address.", stored.name))
            })?,
            token: token.map(SecretString::from),
        },
    };
    McpServer::new(&stored.name, transport)
}

/// The listed servers; none listed yet is an empty list. A file that does not read is an error,
/// so it is never written over with a list that lost its secrets.
pub async fn load_servers(dir: &Path) -> Result<Vec<McpServer>, AssistantError> {
    let bytes = match tokio::fs::read(dir.join(FILE)).await {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => {
            return Err(AssistantError::Config(format!(
                "The MCP server list could not be read: {e}."
            )));
        }
    };
    let file: File = serde_json::from_slice(&bytes)
        .map_err(|e| AssistantError::Config(format!("The MCP server list is not valid: {e}.")))?;
    file.servers.into_iter().map(restored).collect()
}

async fn save_servers(dir: &Path, servers: &[McpServer]) -> Result<(), AssistantError> {
    let file = File {
        servers: servers.iter().map(stored).collect(),
    };
    let bytes = serde_json::to_vec(&file)?;
    tokio::fs::create_dir_all(dir).await?;
    let tmp = dir.join(format!("mcp.{}.tmp", Uuid::new_v4()));
    let written = match write_private(&tmp, &bytes).await {
        Ok(()) => tokio::fs::rename(&tmp, dir.join(FILE)).await,
        Err(e) => Err(e),
    };
    if let Err(e) = written {
        // The temporary file holds secrets and is useless now.
        tokio::fs::remove_file(&tmp).await.ok();
        return Err(e.into());
    }
    Ok(())
}

/// Adds `server` to the list. A name already listed is refused.
pub async fn add_server(dir: &Path, server: McpServer) -> Result<(), AssistantError> {
    let mut servers = load_servers(dir).await?;
    if servers.iter().any(|s| s.name == server.name) {
        return Err(AssistantError::Config(format!(
            "A server called {} is already listed.",
            server.name
        )));
    }
    if servers.len() >= MAX_SERVERS {
        return Err(AssistantError::Config(format!(
            "At most {MAX_SERVERS} servers can be listed."
        )));
    }
    servers.push(server);
    save_servers(dir, &servers).await
}

/// Takes `name` off the list. A name not listed is already gone.
pub async fn remove_server(dir: &Path, name: &str) -> Result<(), AssistantError> {
    let mut servers = load_servers(dir).await?;
    let before = servers.len();
    servers.retain(|s| s.name != name);
    if servers.len() == before {
        return Ok(());
    }
    save_servers(dir, &servers).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(name: &str) -> McpServer {
        let transport = Transport::Stdio {
            command: "tool".into(),
            args: vec!["--flag".into()],
            env: vec![EnvVar {
                name: "KEY".into(),
                value: SecretString::from("hunter2"),
            }],
        };
        McpServer::new(name, transport).unwrap()
    }

    #[test]
    fn names_become_slugs() {
        assert_eq!(slug("  My Notes! "), "my-notes");
        assert_eq!(slug("a__b"), "a-b");
        assert_eq!(slug("---"), "");
    }

    #[tokio::test]
    async fn the_list_survives_privately_and_summaries_hide_secrets() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_servers(dir.path()).await.unwrap().is_empty());
        add_server(dir.path(), command("Files")).await.unwrap();
        let url = Url::parse("https://user:pw@example.test/mcp?key=abc").unwrap();
        let http = McpServer::new(
            "notes",
            Transport::Http {
                url,
                token: Some(SecretString::from("tok")),
            },
        )
        .unwrap();
        add_server(dir.path(), http).await.unwrap();
        assert!(add_server(dir.path(), command("files")).await.is_err());
        let loaded = load_servers(dir.path()).await.unwrap();
        assert_eq!(loaded.len(), 2);
        let summaries: Vec<ServerSummary> = loaded.iter().map(McpServer::summary).collect();
        assert_eq!(summaries[0].detail, "tool --flag");
        assert_eq!(summaries[0].env, ["KEY"]);
        assert_eq!(summaries[1].detail, "https://example.test/mcp");
        assert!(summaries[1].token);
        let shown = serde_json::to_string(&summaries).unwrap();
        assert!(!shown.contains("hunter2") && !shown.contains("tok\"") && !shown.contains("abc"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.path().join(FILE))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        remove_server(dir.path(), "files").await.unwrap();
        assert_eq!(load_servers(dir.path()).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_broken_file_is_an_error_not_an_empty_list() {
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(dir.path().join(FILE), b"{oops")
            .await
            .unwrap();
        assert!(load_servers(dir.path()).await.is_err());
        assert!(add_server(dir.path(), command("a")).await.is_err());
    }
}
