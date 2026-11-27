//! Projects editable fields out of a native entry and patches just those fields.

use anyhow::{Result, ensure};
use beep_proto::{
    SecretText,
    node::mcp_servers::{
        ClaudeOauth, CodexOptions, Connection, Harness, Remote, RemoteTransport, Settings, Stdio,
    },
};
use reqwest::{
    Url,
    header::{HeaderName, HeaderValue},
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use toml_edit::DocumentMut;

#[derive(Default, Deserialize)]
#[serde(default)]
struct NativeFields {
    #[serde(rename = "type")]
    kind: Option<String>,
    command: Option<String>,
    args: Vec<SecretText>,
    env: BTreeMap<String, SecretText>,
    cwd: Option<String>,
    url: Option<SecretText>,
    #[serde(rename = "serverUrl")]
    server_url: Option<SecretText>,
    disabled: Option<bool>,
    headers: BTreeMap<String, SecretText>,
    http_headers: BTreeMap<String, SecretText>,
    env_http_headers: BTreeMap<String, String>,
    bearer_token_env_var: Option<String>,
    enabled: Option<bool>,
    #[serde(flatten)]
    codex: CodexOptions,
    startup_timeout_ms: Option<f64>,
    oauth: Option<ClaudeOauth>,
}

pub(super) fn settings(harness: Harness, entry: &str) -> Option<Settings> {
    let mut native: NativeFields = match harness {
        Harness::Codex => toml::from_str(entry).ok()?,
        Harness::Claude | Harness::Agy => serde_json::from_str(entry).ok()?,
    };
    let url = if harness == Harness::Agy {
        native.server_url
    } else {
        native.url
    };
    let connection = match (native.kind.as_deref(), native.command, url) {
        (None | Some("stdio"), Some(command), None) => Connection::Stdio(Stdio {
            command,
            args: native.args,
            env: native.env,
            cwd: native.cwd,
        }),
        (kind, None, Some(url)) => {
            let transport = match (harness, kind) {
                (Harness::Codex | Harness::Agy, None)
                | (Harness::Claude, Some("http" | "streamable-http")) => RemoteTransport::Http,
                (Harness::Claude, Some("sse")) => RemoteTransport::Sse,
                (Harness::Claude, Some("ws")) => RemoteTransport::Ws,
                _ => return None,
            };
            Connection::Remote(Remote {
                transport,
                url,
                headers: if harness == Harness::Codex {
                    native.http_headers
                } else {
                    native.headers
                },
                env_http_headers: native.env_http_headers,
                bearer_token_env_var: native.bearer_token_env_var,
            })
        }
        _ => return None,
    };
    let codex = harness == Harness::Codex;
    native.codex.startup_timeout_sec = native
        .codex
        .startup_timeout_sec
        .or(native.startup_timeout_ms.map(|ms| ms / 1000.0));
    let claude_oauth = if harness == Harness::Claude
        && matches!(&connection, Connection::Remote(remote) if remote.transport != RemoteTransport::Ws)
    {
        native.oauth
    } else {
        None
    };
    Some(Settings {
        connection,
        enabled: match harness {
            Harness::Codex => native.enabled.unwrap_or(true),
            Harness::Claude => true,
            Harness::Agy => !native.disabled.unwrap_or(false),
        },
        codex: codex.then_some(native.codex),
        claude_oauth,
    })
}

pub(super) fn patch(
    harness: Harness,
    original: Option<(&str, &Settings)>,
    edited: &Settings,
) -> Result<String> {
    validate(harness, edited)?;
    let after = fields(harness, edited)?;
    let before = match original {
        Some((_, previous)) => fields(harness, previous)?,
        None => Map::new(),
    };
    let replaces_http = before.contains_key("url") && !after.contains_key("url");
    let replaces_stdio = before.contains_key("command") && !after.contains_key("command");
    let original = original.map(|(entry, _)| entry);

    match harness {
        Harness::Codex => {
            let mut doc: DocumentMut = original
                .unwrap_or_default()
                .parse()
                .map_err(|_| anyhow::anyhow!("Could not read the existing MCP entry."))?;
            let values: DocumentMut = toml::to_string(&after)?.parse()?;
            for key in before.keys().filter(|key| !after.contains_key(*key)) {
                doc.remove(key);
            }
            for (key, value) in values.iter() {
                if before.get(key) != after.get(key) {
                    doc.insert(key, value.clone());
                }
            }
            if before.get("startup_timeout_sec") != after.get("startup_timeout_sec") {
                doc.remove("startup_timeout_ms");
            }
            if replaces_http {
                for key in ["http_headers_helper", "oauth", "oauth_resource", "scopes"] {
                    doc.remove(key);
                }
            }
            if replaces_stdio {
                doc.remove("env_vars");
            }
            Ok(doc.to_string())
        }
        Harness::Claude | Harness::Agy => {
            let mut entry: Map<String, Value> = match original {
                Some(original) => serde_json::from_str(original)
                    .map_err(|_| anyhow::anyhow!("Could not read the existing MCP entry."))?,
                None => Map::new(),
            };
            let mut after = after;
            let next_oauth = after.remove("oauth");
            for key in before
                .keys()
                .filter(|key| *key != "oauth" && !after.contains_key(*key))
            {
                entry.remove(key);
            }
            for (key, value) in after {
                if before.get(&key) != Some(&value) {
                    entry.insert(key, value);
                }
            }
            let replaces_oauth = matches!(
                before.get("type").and_then(Value::as_str),
                Some("http" | "sse")
            ) && matches!(&edited.connection, Connection::Remote(remote) if remote.transport == RemoteTransport::Ws);
            if replaces_http || replaces_oauth {
                entry.remove("oauth");
            } else {
                patch_oauth(&mut entry, before.get("oauth"), next_oauth.as_ref());
            }
            Ok(serde_json::to_string(&entry)?)
        }
    }
}

fn patch_oauth(entry: &mut Map<String, Value>, before: Option<&Value>, after: Option<&Value>) {
    if before == after {
        return;
    }
    let mut oauth = entry
        .remove("oauth")
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    let previous = before.and_then(Value::as_object).into_iter().flatten();
    let next = after.and_then(Value::as_object);
    for (key, _) in previous.filter(|(key, _)| next.is_none_or(|values| !values.contains_key(*key)))
    {
        oauth.remove(key);
    }
    for (key, value) in next.into_iter().flatten() {
        oauth.insert(key.clone(), value.clone());
    }
    if !oauth.is_empty() {
        entry.insert("oauth".to_owned(), Value::Object(oauth));
    }
}

fn validate(harness: Harness, settings: &Settings) -> Result<()> {
    if harness == Harness::Codex
        && let Some(options) = &settings.codex
    {
        ensure!(
            [options.startup_timeout_sec, options.tool_timeout_sec]
                .into_iter()
                .flatten()
                .all(|seconds| seconds.is_finite() && seconds > 0.0),
            "Enter positive timeout values."
        );
        ensure!(
            options
                .enabled_tools
                .iter()
                .chain(&options.disabled_tools)
                .flatten()
                .all(|name| !name.trim().is_empty()),
            "Enter a name for each tool."
        );
    }
    if harness == Harness::Claude
        && let Some(oauth) = &settings.claude_oauth
    {
        ensure!(
            oauth.callback_port != Some(0),
            "Enter an OAuth callback port between 1 and 65535."
        );
    }
    let variable = |name: &str| !name.trim().is_empty() && !name.contains(['=', '\0']);
    match &settings.connection {
        Connection::Stdio(stdio) => {
            ensure!(
                !stdio.command.trim().is_empty() && !stdio.command.contains('\0'),
                "Enter the command to run."
            );
            ensure!(
                stdio.args.iter().all(|arg| !arg.expose().contains('\0')),
                "An argument contains an invalid character."
            );
            ensure!(
                stdio.env.keys().all(|name| variable(name)),
                "Enter valid environment variable names."
            );
            ensure!(
                stdio
                    .env
                    .values()
                    .all(|value| !value.expose().contains('\0')),
                "An environment variable contains an invalid character."
            );
        }
        Connection::Remote(remote) => {
            if harness != Harness::Claude {
                ensure!(
                    remote.transport == RemoteTransport::Http,
                    "This harness uses Streamable HTTP for remote servers."
                );
            }
            let url = remote.url.expose();
            let scheme = if remote.transport == RemoteTransport::Ws {
                ["ws", "wss"]
            } else {
                ["http", "https"]
            };
            ensure!(
                Url::parse(url).is_ok_and(|url| scheme.contains(&url.scheme()) && url.has_host())
                    || harness == Harness::Claude && url.contains("${"),
                "Enter a URL that matches the server’s transport."
            );
            ensure!(
                remote
                    .headers
                    .keys()
                    .chain(remote.env_http_headers.keys())
                    .all(|name| HeaderName::try_from(name).is_ok()),
                "Enter valid HTTP header names."
            );
            ensure!(
                remote
                    .headers
                    .values()
                    .all(|value| HeaderValue::from_str(value.expose()).is_ok()),
                "An HTTP header value is invalid."
            );
            ensure!(
                remote.env_http_headers.values().all(|name| variable(name))
                    && remote.bearer_token_env_var.as_deref().is_none_or(variable),
                "Enter valid environment variable names."
            );
        }
    }
    Ok(())
}

fn fields(harness: Harness, settings: &Settings) -> Result<Map<String, Value>> {
    let mut values = Map::new();
    match &settings.connection {
        Connection::Stdio(stdio) => {
            match harness {
                Harness::Claude => add(&mut values, "type", "stdio")?,
                Harness::Codex => {
                    if let Some(cwd) = &stdio.cwd {
                        add(&mut values, "cwd", cwd)?;
                    }
                }
                Harness::Agy => {}
            }
            add(&mut values, "command", &stdio.command)?;
            if !stdio.args.is_empty() {
                add(&mut values, "args", &stdio.args)?;
            }
            if !stdio.env.is_empty() {
                add(&mut values, "env", &stdio.env)?;
            }
        }
        Connection::Remote(remote) => {
            add(
                &mut values,
                if harness == Harness::Agy {
                    "serverUrl"
                } else {
                    "url"
                },
                &remote.url,
            )?;
            match harness {
                Harness::Codex => {
                    if !remote.headers.is_empty() {
                        add(&mut values, "http_headers", &remote.headers)?;
                    }
                    if !remote.env_http_headers.is_empty() {
                        add(&mut values, "env_http_headers", &remote.env_http_headers)?;
                    }
                    if let Some(variable) = &remote.bearer_token_env_var {
                        add(&mut values, "bearer_token_env_var", variable)?;
                    }
                }
                Harness::Claude => {
                    add(&mut values, "type", remote.transport.as_ref())?;
                    if !remote.headers.is_empty() {
                        add(&mut values, "headers", &remote.headers)?;
                    }
                }
                Harness::Agy => {
                    if !remote.headers.is_empty() {
                        add(&mut values, "headers", &remote.headers)?;
                    }
                }
            }
        }
    }
    if harness == Harness::Codex {
        if !settings.enabled {
            add(&mut values, "enabled", false)?;
        }
        if let Some(options) = &settings.codex {
            values.extend(present(options)?);
        }
    } else if harness == Harness::Agy {
        if !settings.enabled {
            add(&mut values, "disabled", true)?;
        }
    } else if matches!(&settings.connection, Connection::Remote(remote) if remote.transport != RemoteTransport::Ws)
        && let Some(oauth) = &settings.claude_oauth
    {
        let oauth = present(oauth)?;
        if !oauth.is_empty() {
            values.insert("oauth".to_owned(), Value::Object(oauth));
        }
    }
    Ok(values)
}

fn present(value: impl Serialize) -> Result<Map<String, Value>> {
    let Value::Object(mut fields) = serde_json::to_value(value)? else {
        anyhow::bail!("Expected MCP configuration fields.");
    };
    fields.retain(|_, value| !value.is_null());
    Ok(fields)
}

fn add(values: &mut Map<String, Value>, key: &str, value: impl Serialize) -> Result<()> {
    values.insert(key.to_owned(), serde_json::to_value(value)?);
    Ok(())
}
