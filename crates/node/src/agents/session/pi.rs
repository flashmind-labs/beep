use super::AgentKind;
use anyhow::{Context, Result};
use std::{
    env,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};
use tempfile::TempDir;
use tokio::process::Command;

const PI_COMMAND: &str = "PI_ACP_PI_COMMAND";

/// Route saved instructions through the standalone Pi used by pi-acp.
/// Each session gets a private prompt file and a launcher that adds Pi's
/// native flag to the arguments the adapter already passes.
pub(super) fn instructions(
    kind: &AgentKind,
    process: &mut Command,
    cwd: &Path,
    instructions: &mut Option<String>,
) -> Result<Option<TempDir>> {
    let Some(text) = instructions.as_deref().filter(|_| is_pi_acp(kind)) else {
        return Ok(None);
    };
    let dir = tempfile::tempdir().context("creating Pi's session settings")?;

    // Passing the flag suppresses Pi's discovery of APPEND_SYSTEM.md. Let
    // Pi load its own file, including its project trust check, in that case.
    let agent_dir = value(process, "PI_CODING_AGENT_DIR")
        .map(PathBuf::from)
        .map(|path| cwd.join(path))
        .or_else(|| dirs::home_dir().map(|home| home.join(".pi/agent")));
    let has_append_file = cwd.join(".pi/APPEND_SYSTEM.md").exists()
        || agent_dir.is_some_and(|dir| dir.join("APPEND_SYSTEM.md").exists());
    let (flag, name, contents) = if has_append_file {
        let extension = format!(
            "const instructions = {};\n\
             export default function (pi) {{\n\
                 pi.on('before_agent_start', (event) => ({{\n\
                     systemPrompt: `${{event.systemPrompt}}\\n\\n${{instructions}}`,\n\
                 }}));\n\
             }}\n",
            serde_json::to_string(text)?,
        );
        ("--extension", "instructions.ts", extension)
    } else {
        ("--append-system-prompt", "instructions.md", text.to_owned())
    };
    fs::write(dir.path().join(name), contents).context("writing Pi's session instructions")?;

    let launcher = if cfg!(windows) { "pi.cmd" } else { "pi" };
    let pi = value(process, PI_COMMAND).unwrap_or_else(|| OsString::from(launcher));
    let wrapper = dir.path().join(launcher);
    write_launcher(&wrapper, flag, name).context("preparing Pi's session launcher")?;
    process.env("BEEP_PI_COMMAND", pi).env(PI_COMMAND, wrapper);
    instructions.take();

    Ok(Some(dir))
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
fn write_launcher(path: &Path, flag: &str, name: &str) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::write(
        path,
        format!("#!/bin/sh\nexec \"$BEEP_PI_COMMAND\" {flag} \"${{0%/*}}/{name}\" \"$@\"\n"),
    )?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(windows)]
fn write_launcher(path: &Path, flag: &str, name: &str) -> std::io::Result<()> {
    fs::write(
        path,
        format!(
            "@echo off\r\nsetlocal DisableDelayedExpansion\r\n\"%BEEP_PI_COMMAND%\" {flag} \"%~dp0{name}\" %*\r\n"
        ),
    )
}
