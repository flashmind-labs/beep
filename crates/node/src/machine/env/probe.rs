//! Running a probe command and reading what it printed.

use crate::process::spawn_env::{self, Scope};
use std::{env, path::Path, process::Stdio, time::Duration};
use tokio::time::timeout;

/// How long one probe command may take.
pub(super) const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
/// The longest version line kept.
const MAX_VERSION: usize = 80;

/// What `program args` printed, stdout then stderr, when it ran in time.
pub(crate) async fn output(program: impl AsRef<std::ffi::OsStr>, args: &[&str]) -> Option<String> {
    ran(program, args, Scope::Probe).await.map(|(_, text)| text)
}

/// What `program` printed, stdout then stderr, and whether it succeeded;
/// `None` when it could not run or took too long. It gets the credentials
/// `scope` allows.
pub(super) async fn ran(
    program: impl AsRef<std::ffi::OsStr>,
    args: &[&str],
    scope: Scope,
) -> Option<(bool, String)> {
    let mut command = crate::process::probe(program.as_ref());
    spawn_env::apply(&mut command, scope);
    beside(&mut command, Path::new(program.as_ref()));
    let run = command
        .args(args)
        .stdin(Stdio::null())
        .kill_on_drop(true)
        .output();
    let out = timeout(PROBE_TIMEOUT, run).await.ok()?.ok()?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    Some((out.status.success(), text))
}

/// `program`'s folder first on the `PATH` of `command`, when it was found
/// outside `PATH`, so what it starts (`node` for an npm install) is found.
pub(crate) fn beside(command: &mut tokio::process::Command, program: &Path) {
    let Some(dir) = program.parent().filter(|_| program.is_absolute()) else {
        return;
    };
    let inherited = spawn_env::search_path().unwrap_or_default();
    let dirs = std::iter::once(dir.to_owned()).chain(env::split_paths(&inherited));

    if let Ok(path) = env::join_paths(dirs) {
        command.env("PATH", path);
    }
}

pub(super) fn first_line(text: &str) -> Option<String> {
    let line = text.lines().map(str::trim).find(|line| !line.is_empty())?;
    Some(line.chars().take(MAX_VERSION).collect())
}
