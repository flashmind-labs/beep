//! Edits user-scoped MCP entries in the file the harness already reads.

mod fields;

use super::SERVER_NAME;
use crate::agents::{Agents, kinds};
use crate::logs::found;
use crate::machine::env::locate;
use anyhow::{Context, Result, bail, ensure};
use beep_proto::{
    SecretText,
    node::{LinkRx, LinkTx, mcp_servers::{EditableServer, Harness, NativeServer, Reply, Request, Server}},
};
use serde_json::{Map, Value};
use std::{
    env,
    fs::{self, File},
    io::{ErrorKind, Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tokio::task::spawn_blocking;
use toml_edit::{DocumentMut, Item, Table};

const HARNESSES: [Harness; 3] = [Harness::Codex, Harness::Claude, Harness::Agy];
const MAX_CONFIG: u64 = 8 * 1024 * 1024;
const MAX_SERVER: usize = 64 * 1024;
static WRITING: Mutex<()> = Mutex::new(());

pub(crate) async fn serve(agents: Arc<Agents>, mut tx: LinkTx, mut rx: LinkRx) -> Result<()> {
    let Some(request) = rx.recv::<Request>().await? else {
        return Ok(());
    };

    let reply = spawn_blocking(move || answer(&agents, request)).await?;
    tx.send(&reply.unwrap_or_else(|error| Reply::Refused(error.to_string()))).await?;
    tx.finish().await
}

fn answer(agents: &Agents, request: Request) -> Result<Reply> {
    match request {
        Request::Harnesses => Ok(Reply::Harnesses(HARNESSES.into_iter().filter(|&h| config_path(agents, h).is_some()).collect())),
        Request::List { harness } => {
            let path = available(agents, harness)?;
            Ok(Reply::Servers(Document::parse(harness, &read(&path)?)?.servers(harness)?))
        }
        Request::Save { harness, previous, server } => {
            change(&available(agents, harness)?, harness, Change::Save { previous, server })
        }
        Request::Remove { harness, name } => {
            change(&available(agents, harness)?, harness, Change::Remove(name))
        }
        Request::Export { harness, name } => {
            let path = available(agents, harness)?;
            let server = Document::parse(harness, &read(&path)?)?.native(&name)?
                .context("That MCP server was removed.")?;
            Ok(Reply::NativeServer(server))
        }
        Request::Apply { harness, server } => {
            change(&available(agents, harness)?, harness, Change::Apply(server))
        }
    }
}

fn available(agents: &Agents, harness: Harness) -> Result<PathBuf> {
    config_path(agents, harness).context("This machine has no supported local configuration for that harness.")
}

fn config_path(agents: &Agents, harness: Harness) -> Option<PathBuf> {
    let home = dirs::home_dir();
    let kind = agents.named(harness.as_ref());
    let native = kind.as_ref().filter(|kind| !kinds::in_wsl(kind));
    let override_dir = |key| native.and_then(|kind| kind.env.get(key)).filter(|value| !value.is_empty()).map(PathBuf::from);
    let path = match harness {
        Harness::Codex => override_dir("CODEX_HOME")
            .or_else(found::codex::configured)?
            .join("config.toml"),
        Harness::Claude => override_dir("CLAUDE_CONFIG_DIR")
            .or_else(|| env::var_os("CLAUDE_CONFIG_DIR").filter(|dir| !dir.is_empty()).map(PathBuf::from))
            .or(home.clone())?
            .join(".claude.json"),
        // Nothing but its home moves Antigravity's configuration.
        Harness::Agy => override_dir("HOME")
            .or(home.clone())?
            .join(".gemini/config/mcp_config.json"),
    };

    if !path.is_absolute() {
        return None;
    }
    let installed = native.is_some() || locate::find(harness.as_ref(), home.as_deref()).is_some();
    (installed || path.is_file()).then_some(path)
}

enum Change {
    Save { previous: Option<Box<EditableServer>>, server: Box<EditableServer> },
    Remove(String),
    Apply(NativeServer),
}

fn change(path: &Path, harness: Harness, change: Change) -> Result<Reply> {
    let _writing = crate::lock(&WRITING);
    let path = match fs::canonicalize(path) {
        Ok(path) => path,
        Err(error) if error.kind() == ErrorKind::NotFound && !path.is_symlink() => path.to_owned(),
        Err(_) => bail!("The harness configuration file could not be opened."),
    };
    let original = read(&path)?;
    let mut doc = Document::parse(harness, &original)?;
    let apply = matches!(change, Change::Apply(_));
    let changed = match change {
        Change::Save { previous, server } => {
            let original = if let Some(previous) = &previous {
                let entry = doc.native(&previous.name)?;
                let current = entry.as_ref().and_then(|entry| fields::settings(harness, entry.config.expose()));
                if current.as_ref() != Some(&previous.settings) {
                    return Ok(Reply::Conflict(doc.servers(harness)?));
                }
                entry
            } else {
                None
            };

            let previous_name = previous.as_ref().map(|previous| previous.name.as_str());
            if previous_name != Some(server.name.as_str()) && doc.contains(&server.name)? {
                bail!("An MCP server already has that name.");
            }

            let original = original.as_ref().zip(previous.as_ref())
                .map(|(entry, previous)| (entry.config.expose(), &previous.settings));
            let config = fields::patch(harness, original, &server.settings)?;
            doc.put(&NativeServer { name: server.name, config: SecretText::new(config) }, previous_name)?
        }
        Change::Remove(name) => doc.remove(&name)?,
        Change::Apply(server) => doc.put(&server, None)?,
    };

    let servers = doc.servers(harness)?;
    if changed {
        write(&path, &original, &doc.text()?)?;
    }

    Ok(if apply { Reply::Applied } else { Reply::Servers(servers) })
}

enum Document {
    Codex(DocumentMut),
    /// A JSON file with the servers under `mcpServers`, and the name of the
    /// harness it belongs to.
    Json(&'static str, Map<String, Value>),
}

impl Document {
    fn parse(harness: Harness, original: &str) -> Result<Self> {
        match harness {
            Harness::Codex => original.parse().map(Self::Codex).map_err(|_| anyhow::anyhow!("Codex's configuration is invalid TOML.")),
            Harness::Claude | Harness::Agy => {
                let label = if harness == Harness::Claude { "Claude Code" } else { "Antigravity" };
                if original.is_empty() {
                    return Ok(Self::Json(label, Map::new()));
                }

                serde_json::from_str(original).map(|doc| Self::Json(label, doc))
                    .map_err(|_| anyhow::anyhow!("{label}'s configuration is invalid JSON."))
            }
        }
    }

    fn servers(&self, harness: Harness) -> Result<Vec<Server>> {
        let mut servers = self.names()?.into_iter().map(|name| {
            let settings = self.native(name)?.and_then(|entry| fields::settings(harness, entry.config.expose()));
            Ok(Server { name: name.to_owned(), settings })
        }).collect::<Result<Vec<_>>>()?;
        servers.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(servers)
    }

    fn names(&self) -> Result<Vec<&str>> {
        match self {
            Self::Codex(doc) => Ok(match doc.get("mcp_servers") {
                Some(item) => item.as_table_like().context("Codex's mcp_servers must be a table.")?
                    .iter().map(|(name, _)| name).collect(),
                None => Vec::new(),
            }),
            Self::Json(label, doc) => Ok(match doc.get("mcpServers") {
                Some(item) => item.as_object().with_context(|| format!("{label}'s mcpServers must be an object."))?
                    .keys().map(String::as_str).collect(),
                None => Vec::new(),
            }),
        }
    }

    fn native(&self, name: &str) -> Result<Option<NativeServer>> {
        let config = match self {
            Self::Codex(doc) => doc.get("mcp_servers").and_then(Item::as_table_like)
                .and_then(|table| table.get(name))
                .and_then(|item| item.as_table().cloned().or_else(|| item.as_inline_table().cloned().map(|table| table.into_table())))
                .map(|table| DocumentMut::from(table).to_string()),
            Self::Json(_, doc) => doc.get("mcpServers").and_then(Value::as_object)
                .and_then(|servers| servers.get(name))
                .map(serde_json::to_string)
                .transpose()?,
        };
        Ok(config.map(|config| NativeServer { name: name.to_owned(), config: SecretText::new(config) }))
    }

    fn contains(&self, name: &str) -> Result<bool> {
        Ok(match self {
            Self::Codex(doc) => doc.get("mcp_servers").map(|item|
                item.as_table_like().context("Codex's mcp_servers must be a table.")
                    .map(|table| table.contains_key(name))
            ).transpose()?.unwrap_or_default(),
            Self::Json(label, doc) => doc.get("mcpServers").map(|item|
                item.as_object().with_context(|| format!("{label}'s mcpServers must be an object."))
                    .map(|object| object.contains_key(name))
            ).transpose()?.unwrap_or_default(),
        })
    }

    fn put(&mut self, server: &NativeServer, previous: Option<&str>) -> Result<bool> {
        if previous != Some(server.name.as_str()) {
            ensure!(
                !server.name.is_empty() && server.name.len() <= 128 &&
                    server.name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_')),
                "Use up to 128 letters, numbers, hyphens or underscores for the MCP server name."
            );
        }
        ensure!(!server.name.eq_ignore_ascii_case(SERVER_NAME), "Beep is a reserved MCP server name.");
        let config = server.config.expose();
        ensure!(config.len() <= MAX_SERVER, "The MCP server entry is too large.");

        if let Some(previous) = previous
            && previous != server.name
        {
            self.remove(previous)?;
        }
        match self {
            Self::Codex(doc) => {
                let entry: DocumentMut = config.parse()
                    .map_err(|_| anyhow::anyhow!("The Codex MCP entry is invalid."))?;
                validate_transport(entry.get("command").and_then(Item::as_str), entry.get("url").and_then(Item::as_str))?;
                let item = doc.as_table_mut().entry("mcp_servers").or_insert_with(|| {
                    let mut table = Table::new();
                    table.set_implicit(true);
                    Item::Table(table)
                });
                let inline = item.is_inline_table();
                let servers = item.as_table_like_mut().context("Codex's mcp_servers must be a table.")?;
                let table = entry.into_table();
                let value = if inline { Item::Value(table.into_inline_table().into()) } else { Item::Table(table) };
                servers.insert(&server.name, value);
            }
            Self::Json(label, doc) => {
                let entry: Map<String, Value> = serde_json::from_str(config)
                    .map_err(|_| anyhow::anyhow!("The {label} MCP entry is invalid."))?;
                // Antigravity names its URL `serverUrl` and has one remote transport.
                let url = entry.get("url").or_else(|| entry.get("serverUrl")).and_then(Value::as_str);
                validate_transport(entry.get("command").and_then(Value::as_str), url)?;
                if entry.contains_key("url") {
                    ensure!(
                        entry.get("type").and_then(Value::as_str).is_some_and(|kind| !kind.trim().is_empty()),
                        "Specify the transport type for a {label} MCP server with a URL."
                    );
                }
                let item = doc.entry("mcpServers").or_insert_with(|| Value::Object(Map::new()));
                let servers = item.as_object_mut().with_context(|| format!("{label}'s mcpServers must be an object."))?;
                servers.insert(server.name.clone(), Value::Object(entry));
            }
        }

        Ok(true)
    }

    fn remove(&mut self, name: &str) -> Result<bool> {
        if !self.contains(name)? {
            return Ok(false);
        }
        Ok(match self {
            Self::Codex(doc) => doc.get_mut("mcp_servers").and_then(Item::as_table_like_mut)
                .and_then(|table| table.remove(name)).is_some(),
            Self::Json(_, doc) => doc.get_mut("mcpServers").and_then(Value::as_object_mut)
                .and_then(|object| object.remove(name)).is_some(),
        })
    }

    fn text(&self) -> Result<String> {
        match self {
            Self::Codex(doc) => Ok(doc.to_string()),
            Self::Json(_, doc) => Ok(format!("{}\n", serde_json::to_string_pretty(doc)?)),
        }
    }
}

fn validate_transport(command: Option<&str>, url: Option<&str>) -> Result<()> {
    let present = |value: Option<&str>| value.is_some_and(|value| !value.trim().is_empty());
    ensure!(present(command) != present(url), "Enter either a command or a URL for the MCP server.");
    Ok(())
}

fn read(path: &Path) -> Result<String> {
    let mut text = String::new();
    match File::open(path) {
        Ok(file) => {
            file.take(MAX_CONFIG + 1).read_to_string(&mut text)
                .context("Could not read the harness configuration.")?;
            ensure!(text.len() as u64 <= MAX_CONFIG, "The harness configuration is too large.");
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(_) => bail!("Could not open the harness configuration."),
    }

    Ok(text)
}

fn write(path: &Path, original: &str, edited: &str) -> Result<()> {
    if original == edited {
        return Ok(());
    }
    let dir = path.parent().context("The harness configuration has no parent folder.")?;
    fs::create_dir_all(dir).context("Could not create the harness configuration folder.")?;
    if let Ok(metadata) = fs::metadata(path) {
        ensure!(!metadata.permissions().readonly(), "The harness configuration is read only.");
    }
    let mut temporary = tempfile::NamedTempFile::new_in(dir).context("Could not prepare the harness configuration.")?;
    temporary.write_all(edited.as_bytes()).context("Could not write the harness configuration.")?;
    temporary.as_file().sync_all().context("Could not save the harness configuration.")?;
    ensure!(read(path)? == original, "The harness configuration changed. Reopen the harness and try again.");
    temporary.persist(path).context("Could not replace the harness configuration.")?;

    Ok(())
}

#[cfg(test)]
#[path = "../tests/tools_configured.rs"]
mod tests;
