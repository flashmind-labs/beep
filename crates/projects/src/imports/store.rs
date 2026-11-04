//! What an import reads and writes about the user's projects, kept by the server in Postgres and
//! by the desktop in its mesh state.

use super::records::{ImportRow, NewImport};
use crate::model::RepoName;
use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
pub trait ImportStore: Send + Sync {
    /// The user's imports, by project, then machine.
    async fn imports(&self) -> Result<Vec<ImportRow>>;

    /// Records an import, or moves it to its folder, making the project if new.
    async fn save_import(&self, import: NewImport<'_>) -> Result<ImportRow>;

    /// Records a found checkout as the project's folder on that machine unless it has one.
    async fn link_import(&self, import: NewImport<'_>) -> Result<bool>;

    /// Notes the fork the import of `project` on `endpoint` cloned, or none.
    async fn set_import_fork(
        &self,
        project: &str,
        endpoint: &str,
        fork: Option<&RepoName>,
    ) -> Result<()>;
}
