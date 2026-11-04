//! A machine of the user's as the import code sees it: its folders, a shell, and what it can say
//! about the coding agents' folders.

use super::place::Folders;
use crate::Shell;
use anyhow::Result;
use async_trait::async_trait;
use beep_proto::node::agent::SessionFolder;
use std::sync::Arc;

/// A machine that can name the folders its coding agents worked in.
#[async_trait]
pub trait FoundFolders: Send + Sync {
    /// At most `limit` folders, newest activity first.
    async fn found(&self, limit: u32) -> Result<Vec<SessionFolder>>;
}

/// One of the user's machines.
#[derive(Clone)]
pub struct Machine {
    /// The endpoint id, as the store keeps it.
    pub id: String,
    pub name: String,
    pub online: bool,
    /// `linux`, `macos` or `windows`; `None` before its first report.
    pub os: Option<String>,
    pub home: Option<String>,
    /// The user's files and code, on the user's own Linux environment.
    pub workspace: Option<String>,
    /// A shell on it, while it is online and runs commands.
    pub shell: Option<Arc<dyn Shell>>,
    /// Set when it is online and can say which folders its agents worked in.
    pub sessions: Option<Arc<dyn FoundFolders>>,
}

impl Machine {
    pub fn folders(&self) -> Folders<'_> {
        Folders {
            os: self.os.as_deref(),
            home: self.home.as_deref(),
            workspace: self.workspace.as_deref(),
        }
    }

    /// The shell, for a machine already chosen as one that has it.
    pub fn shell(&self) -> Result<&dyn Shell, super::ImportError> {
        self.shell
            .as_deref()
            .ok_or_else(|| super::ImportError::NoShell(self.name.clone()))
    }
}
