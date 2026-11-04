//! Moving an account that has a server into the mesh: the reads are the server's, the work is
//! `beep_fleet::migrate`. It runs in the background once the mesh is open and a server is linked,
//! and again on the next launch if the server did not answer; Home never waits for it.

use crate::{
    beep::{Beep, state::LinkState},
    center::home_sessions::HOME_CHANGED,
    error::DesktopError,
    fleet::Fleet,
};
use async_trait::async_trait;
use beep_fleet::{
    FleetError, Result,
    migrate::{self, Source},
};
use beep_proto::{
    Credential, FlshSettings, GetFlshSettings, ImportedProject, InstructionsVersion,
    ListCredentials, ListHarnessSettings, ListImportedProjects, ListInstructionsVersions,
    ListProjectGroups, ListProjectSync, ListProjectsOnTop, MachineHarnessSettings, ProjectGroup,
    ProjectSync,
};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};

fn failed(e: DesktopError) -> FleetError {
    FleetError::Other(anyhow::anyhow!("{e}"))
}

/// The account's server, as the move reads it.
struct Server<'a> {
    beep: &'a Beep,
}

#[async_trait]
impl Source for Server<'_> {
    async fn imports(&self) -> Result<Vec<ImportedProject>> {
        self.beep
            .call(ListImportedProjects {})
            .await
            .map_err(failed)
    }

    async fn sync(&self) -> Result<Vec<ProjectSync>> {
        self.beep.call(ListProjectSync {}).await.map_err(failed)
    }

    async fn groups(&self) -> Result<Vec<ProjectGroup>> {
        self.beep.call(ListProjectGroups {}).await.map_err(failed)
    }

    async fn on_top(&self) -> Result<Vec<String>> {
        self.beep.call(ListProjectsOnTop {}).await.map_err(failed)
    }

    async fn credentials(&self) -> Result<Vec<Credential>> {
        self.beep.call(ListCredentials {}).await.map_err(failed)
    }

    async fn harness(&self, agent: &str) -> Result<Vec<MachineHarnessSettings>> {
        let request = ListHarnessSettings {
            agent: agent.to_owned(),
        };
        self.beep.call(request).await.map_err(failed)
    }

    async fn flsh(&self) -> Result<FlshSettings> {
        self.beep.call(GetFlshSettings {}).await.map_err(failed)
    }

    /// Oldest first, as the move wants them; the server lists the newest first.
    async fn instructions(&self) -> Result<Vec<InstructionsVersion>> {
        let mut versions = self
            .beep
            .call(ListInstructionsVersions {})
            .await
            .map_err(failed)?;
        versions.reverse();
        Ok(versions)
    }
}

/// Moves this account into the mesh, if it has a server it is signed in to and the move did not
/// finish before. A server that does not answer is logged, and the next launch tries again.
pub(crate) async fn run(app: AppHandle) {
    let beep = app.state::<Arc<Beep>>().inner().clone();
    let fleet = app.state::<Arc<Fleet>>().inner().clone();
    let Some(server) = beep.server() else {
        return;
    };
    if beep.link_state() != LinkState::Linked {
        return;
    }
    let mesh = match fleet.ready().await {
        Ok(mesh) => mesh,
        Err(e) => {
            log::warn!("the account is not moved to the mesh: {e}");
            return;
        }
    };
    match migrate::run(&mesh, &Server { beep: &beep }, &server.to_string()).await {
        Ok(report) if report.already => {}
        Ok(report) => {
            log::info!(
                "moved the account to the mesh: {} written, {} credentials to enter again",
                report.written,
                report.unmoved
            );
            if let Err(e) = app.emit(HOME_CHANGED, ()) {
                log::warn!("telling the windows the account moved: {e}");
            }
        }
        Err(e) => log::warn!("the account will move on the next launch: {e}"),
    }
}
