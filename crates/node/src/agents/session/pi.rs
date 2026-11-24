use super::AgentKind;
use agent_client_protocol::v1::schema::McpServer;
use anyhow::{Context, Result};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    env,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};
use tempfile::TempDir;
use tokio::process::Command;

const PI_COMMAND: &str = "PI_ACP_PI_COMMAND";
/// Match Beep's longest MCP tool waits (e.g. `remote_run` and `ask_user`).
const MCP_TIMEOUT_SECS: u32 = 600;

/// Configure the standalone Pi process used by pi-acp.
/// Each session gets a private launcher that loads Beep's session extension
/// (MCP registration and error recovery) and routes saved instructions through
/// Pi's native flags.
pub(super) fn setup(
    kind: &AgentKind,
    process: &mut Command,
    cwd: &Path,
    instructions: &mut Option<String>,
    mcp: Option<&McpServer>,
) -> Result<Option<TempDir>> {
    if !is_pi_acp(kind) {
        return Ok(None);
    }
    let dir = tempfile::tempdir().context("creating Pi's session settings")?;

    // Passing --append-system-prompt suppresses Pi's discovery of
    // APPEND_SYSTEM.md. Route saved instructions through the extension instead
    // when that file exists so Pi keeps loading it with its trust check.
    let agent_dir = value(process, "PI_CODING_AGENT_DIR")
        .map(PathBuf::from)
        .map(|path| cwd.join(path))
        .or_else(|| dirs::home_dir().map(|home| home.join(".pi/agent")));
    let has_append_file = cwd.join(".pi/APPEND_SYSTEM.md").exists()
        || agent_dir.is_some_and(|dir| dir.join("APPEND_SYSTEM.md").exists());

    let mut flags: Vec<(&str, &str)> = Vec::new();
    let inline_instructions = match instructions.as_deref() {
        Some(text) if has_append_file => Some(text),
        Some(text) => {
            fs::write(dir.path().join("instructions.md"), text)
                .context("writing Pi's session instructions")?;
            flags.push(("--append-system-prompt", "instructions.md"));
            None
        }
        None => None,
    };

    let extension = extension_source(inline_instructions, mcp)?;
    let ext_path = dir.path().join("beep.ts");
    write_private(&ext_path, &extension).context("writing Pi's session extension")?;
    flags.push(("--extension", "beep.ts"));

    let launcher = if cfg!(windows) { "pi.cmd" } else { "pi" };
    let pi = value(process, PI_COMMAND).unwrap_or_else(|| OsString::from(launcher));
    let wrapper = dir.path().join(launcher);
    write_launcher(&wrapper, &flags).context("preparing Pi's session launcher")?;
    process.env("BEEP_PI_COMMAND", pi).env(PI_COMMAND, wrapper);
    instructions.take();

    Ok(Some(dir))
}

#[derive(Serialize)]
struct PiMcpServer<'a> {
    name: &'a str,
    config: PiMcpConfig<'a>,
}

#[derive(Serialize)]
struct PiMcpConfig<'a> {
    command: String,
    args: &'a [String],
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    env: BTreeMap<&'a str, &'a str>,
    exposure: &'static str,
    timeout: u32,
}

fn mcp_server_spec(mcp: Option<&McpServer>) -> Option<PiMcpServer<'_>> {
    let McpServer::Stdio(stdio) = mcp? else {
        return None;
    };
    let env = stdio
        .env
        .iter()
        .map(|var| (var.name.as_str(), var.value.as_str()))
        .collect();

    Some(PiMcpServer {
        name: &stdio.name,
        config: PiMcpConfig {
            command: stdio.command.to_string_lossy().into_owned(),
            args: &stdio.args,
            env,
            exposure: "direct",
            timeout: MCP_TIMEOUT_SECS,
        },
    })
}

fn extension_source(instructions: Option<&str>, mcp: Option<&McpServer>) -> Result<String> {
    let instructions_json = match instructions {
        Some(text) => serde_json::to_string(text)?,
        None => "null".to_owned(),
    };
    let mcp_json = match mcp_server_spec(mcp) {
        Some(spec) => serde_json::to_string(&spec)?,
        None => "null".to_owned(),
    };

    Ok(format!(
        r#"const instructions = {instructions_json};
const mcpServer = {mcp_json};
const MAX_CONTENT_FILTER_RETRIES = 2;

export default function (pi) {{
    if (mcpServer !== null && typeof pi.registerMcpServer === 'function') {{
        const registered =
            typeof pi.getMcpServers === 'function'
                ? pi.getMcpServers().some((server) => server?.name === mcpServer.name)
                : false;
        if (!registered) {{
            try {{
                pi.registerMcpServer(mcpServer.name, mcpServer.config);
            }} catch {{
                // Leave Pi running if another extension or config already registered it.
            }}
        }}
    }}

    let contentFilterRetries = 0;
    let unrecoveredError = null;

    pi.on('before_agent_start', (event) => {{
        contentFilterRetries = 0;
        unrecoveredError = null;
        if (instructions !== null) {{
            return {{
                systemPrompt: `${{event.systemPrompt}}\n\n${{instructions}}`,
            }};
        }}
    }});

    pi.on('agent_start', () => {{
        unrecoveredError = null;
    }});

    pi.on('agent_end', (event) => {{
        const messages = Array.isArray(event?.messages) ? event.messages : [];
        let last = null;
        for (let i = messages.length - 1; i >= 0; i -= 1) {{
            if (messages[i]?.role === 'assistant') {{
                last = messages[i];
                break;
            }}
        }}
        if (!last || last.stopReason !== 'error') {{
            unrecoveredError = null;
            return;
        }}

        const rawStopReason = typeof last.rawStopReason === 'string' ? last.rawStopReason : '';
        const errorMessage = typeof last.errorMessage === 'string' ? last.errorMessage.trim() : '';
        const isContentFilter =
            rawStopReason === 'content_filter' || /\bcontent_filter\b/i.test(errorMessage);

        if (isContentFilter && contentFilterRetries < MAX_CONTENT_FILTER_RETRIES) {{
            contentFilterRetries += 1;
            const tools = Array.isArray(last.content)
                ? [
                      ...new Set(
                          last.content
                              .filter((block) => block?.type === 'toolCall' && typeof block.name === 'string')
                              .map((block) => block.name),
                      ),
                  ]
                : [];
            const target =
                tools.length > 0
                    ? `Your previous response and its pending tool call(s) (${{tools.join(', ')}}) were blocked`
                    : 'Your previous response was blocked';
            pi.sendMessage(
                {{
                    customType: 'beep-error-recovery',
                    content: `${{target}} by the provider's content filter (content_filter) and did not run. Do not repeat that command or wording; either try a different, safe approach or explain to the user why you cannot perform it.`,
                    display: false,
                }},
                {{ triggerTurn: true, deliverAs: 'followUp' }},
            );
            return;
        }}

        unrecoveredError = errorMessage || 'The model request failed.';
    }});

    pi.on('agent_settled', (event, ctx) => {{
        if (event?.aborted || !unrecoveredError) {{
            unrecoveredError = null;
            return;
        }}
        const message = /^error\b/i.test(unrecoveredError)
            ? unrecoveredError
            : `Error: ${{unrecoveredError}}`;
        unrecoveredError = null;
        ctx?.ui?.notify?.(message, 'error');
    }});
}}
"#
    ))
}

fn value(process: &Command, name: &str) -> Option<OsString> {
    process
        .as_std()
        .get_envs()
        .find(|(key, _)| *key == name)
        .map_or_else(|| env::var_os(name), |(_, value)| value.map(OsString::from))
}

fn is_pi_acp(kind: &AgentKind) -> bool {
    if kind.name != "pi" || kind.bridge.is_some() {
        return false;
    }

    match Path::new(&kind.program)
        .file_stem()
        .and_then(|stem| stem.to_str())
    {
        Some("pi-acp") => true,
        Some("npx") => kind
            .args
            .iter()
            .any(|arg| arg == "pi-acp" || arg.starts_with("pi-acp@")),
        Some("node") => kind.args.first().is_some_and(|script| {
            Path::new(script)
                .ancestors()
                .any(|path| path.ends_with("node_modules/pi-acp"))
        }),
        _ => false,
    }
}

#[cfg(unix)]
fn write_private(path: &Path, contents: &str) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::write(path, contents)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn write_private(path: &Path, contents: &str) -> std::io::Result<()> {
    fs::write(path, contents)
}

#[cfg(unix)]
fn write_launcher(path: &Path, flags: &[(&str, &str)]) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let extra: String = flags
        .iter()
        .map(|(flag, name)| format!(" {flag} \"${{0%/*}}/{name}\""))
        .collect();
    fs::write(
        path,
        format!("#!/bin/sh\nexec \"$BEEP_PI_COMMAND\"{extra} \"$@\"\n"),
    )?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(windows)]
fn write_launcher(path: &Path, flags: &[(&str, &str)]) -> std::io::Result<()> {
    let extra: String = flags
        .iter()
        .map(|(flag, name)| format!(" {flag} \"%~dp0{name}\""))
        .collect();
    fs::write(
        path,
        format!(
            "@echo off\r\nsetlocal DisableDelayedExpansion\r\n\"%BEEP_PI_COMMAND%\"{extra} %*\r\n"
        ),
    )
}
