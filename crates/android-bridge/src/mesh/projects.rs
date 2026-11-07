use super::Connected;
use crate::{BeepError, bad_request};
use beep_proto::{ProjectAgent, WorkDiff, node::agent::DiffAsk};
use std::sync::Arc;
use uuid::Uuid;

impl Connected {
    pub fn projects(&self) -> Vec<ProjectAgent> {
        beep_fleet::project_names(&self.mesh)
            .into_iter()
            .map(|name| ProjectAgent {
                project_id: beep_fleet::project_id(&name),
                name,
                chat_id: Uuid::nil(),
                open_tasks: 0,
                open_workers: 0,
            })
            .collect()
    }

    pub async fn project_name(self: &Arc<Self>, id: &str) -> Result<String, BeepError> {
        let id = Uuid::parse_str(id).map_err(|_| bad_request("Invalid project identity"))?;
        self.desk()
            .await?
            .project_name(id)
            .ok_or(BeepError::NotFound)
    }
}

impl Connected {
    pub async fn diff(&self, machine: &str, root: &str, head: &str) -> Result<WorkDiff, BeepError> {
        use beep_mesh::machine::sessions::{self, NodeWork};
        let client = self.client(machine).await?;
        match sessions::diff(
            &client,
            DiffAsk {
                root: root.into(),
                base: String::new(),
                head: head.into(),
                only: vec![],
                lockfiles: false,
            },
        )
        .await?
        {
            NodeWork::Done(diff) => Ok(diff),
            NodeWork::Refused(reason) => Err(bad_request(reason)),
        }
    }
}
