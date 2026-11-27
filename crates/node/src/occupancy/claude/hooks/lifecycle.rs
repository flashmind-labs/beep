//! Claude Code hooks installed for the lifetime of a standalone node.

use super::{hook_command, install, read_settings, remove_matching, user_settings, write_settings};
use anyhow::Result;
use std::path::{Path, PathBuf};

/// Removes this node's hook command when the node stops, including when
/// startup fails. Cleanup reads the settings again to keep concurrent edits.
pub struct Installation {
    settings: PathBuf,
    command: String,
}

impl Installation {
    /// Installs hooks pointing to the running node's executable and folder.
    pub fn for_node(dir: &Path) -> Result<Self> {
        let exe = std::env::current_exe()?;
        let dir = std::path::absolute(dir)?;
        Self::at(user_settings()?, hook_command(&exe, &dir))
    }

    fn at(settings: PathBuf, command: String) -> Result<Self> {
        install(&settings, &command)?;
        Ok(Self { settings, command })
    }

    fn remove(&self) -> Result<()> {
        let mut settings = read_settings(&self.settings)?;
        let removed = remove_matching(&mut settings, |handler| {
            handler["command"].as_str() == Some(self.command.as_str())
        });

        if removed > 0 {
            write_settings(&self.settings, &settings)?;
        }

        Ok(())
    }
}

impl Drop for Installation {
    fn drop(&mut self) {
        if let Err(e) = self.remove() {
            log::warn!("could not remove this node's Claude Code hooks: {e:#}");
        }
    }
}

#[cfg(test)]
#[path = "../../../tests/occupancy_claude_hooks_lifecycle.rs"]
mod tests;
