//! A project as the shared project code reads it from Postgres.

use crate::{account::user::User, store::ProjectRow};
use anyhow::Result;
use async_trait::async_trait;
use beep_projects::{
    issues::{ImportedPlace, ProjectStore},
    model::{IssueHost, RepoName},
};
use iroh::EndpointId;

pub(crate) struct StoredProject<'a> {
    user: &'a User,
    project: &'a ProjectRow,
}

impl<'a> StoredProject<'a> {
    pub(crate) fn new(user: &'a User, project: &'a ProjectRow) -> Self {
        Self { user, project }
    }
}

#[async_trait]
impl ProjectStore for StoredProject<'_> {
    fn name(&self) -> &RepoName {
        &self.project.name
    }

    fn issues_role(&self) -> Option<&IssueHost> {
        self.project.roles.issues.as_ref()
    }

    async fn home_machine(&self) -> Result<Option<EndpointId>> {
        let checkouts = self.user.checkouts(self.project.id).await?;
        Ok(checkouts
            .iter()
            .find(|checkout| checkout.home)
            .map(|checkout| checkout.machine))
    }

    async fn imports(&self) -> Result<Vec<ImportedPlace>> {
        let name = self.project.name.to_string();
        Ok(self
            .user
            .imports()
            .await?
            .into_iter()
            .filter(|import| import.project.eq_ignore_ascii_case(&name))
            .map(|import| ImportedPlace {
                machine: import.endpoint.parse().ok(),
                url: import.url,
            })
            .collect())
    }

    async fn tracked_issues(&self, host: &IssueHost) -> Result<Vec<u64>> {
        Ok(self
            .user
            .project_tasks(self.project.id)
            .await?
            .into_iter()
            .filter(|task| task.issue_host.as_ref() == Some(host))
            .filter_map(|task| task.issue.map(|issue| issue.number))
            .collect())
    }
}
