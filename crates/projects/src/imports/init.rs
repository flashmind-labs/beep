//! Making a folder a new git repository on a machine.

use super::ImportError;
use crate::{Shell, quote};
use std::time::Duration;

/// The longest `git init` may take.
const INIT_LIMIT: Duration = Duration::from_secs(60);
/// What the init script prints last, before what it found.
const MARK: &str = "BEEP_INIT:";

/// Makes `folder` a new repository on the machine named `machine`, unless something is there.
pub async fn init(shell: &dyn Shell, machine: &str, folder: &str) -> Result<(), ImportError> {
    let ran = shell
        .run(&init_script(folder), INIT_LIMIT)
        .await
        .map_err(|e| ImportError::Link {
            machine: machine.to_owned(),
            detail: format!("{e:#}"),
        })?;
    let said = ran
        .output
        .lines()
        .rev()
        .find_map(|line| line.trim_end_matches('\r').strip_prefix(MARK));
    match said.map(str::trim) {
        Some("made") => Ok(()),
        Some("occupied") => Err(ImportError::Occupied {
            folder: folder.to_owned(),
            machine: machine.to_owned(),
        }),
        Some("nogit") => Err(ImportError::NoGit(machine.to_owned())),
        _ => Err(ImportError::InitFailed {
            machine: machine.to_owned(),
            detail: super::clone::tail(&ran.output),
        }),
    }
}

fn init_script(folder: &str) -> String {
    format!(
        "d={folder}\n\
         if [ -n \"$(ls -A \"$d\" 2>/dev/null)\" ]; then\n\
         \x20 printf '\\n{MARK} occupied\\n'\n\
         elif ! command -v git >/dev/null 2>&1; then\n\
         \x20 printf '\\n{MARK} nogit\\n'\n\
         elif mkdir -p \"$d\" && git -C \"$d\" init --quiet; then\n\
         \x20 printf '\\n{MARK} made\\n'\n\
         fi\n",
        folder = quote(folder),
    )
}
