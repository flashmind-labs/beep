use crate::{BeepClient, BeepError};
use beep_mesh::machine::agent_files;
use beep_proto::{
    GetInstructions, InstructionsVersion, ListInstructionsVersions, MachineAgentFiles,
    ReadAgentFiles, SaveInstructions,
};

#[uniffi::export(async_runtime = "tokio")]
impl BeepClient {
    pub async fn instructions(&self) -> Result<Option<InstructionsVersion>, BeepError> {
        if let Some(mesh) = self.mesh().await? {
            return Ok(mesh.desk().await?.instructions());
        }
        self.call(GetInstructions {}).await
    }

    pub async fn instructions_history(&self) -> Result<Vec<InstructionsVersion>, BeepError> {
        if let Some(mesh) = self.mesh().await? {
            return Ok(mesh.desk().await?.instructions_versions());
        }
        self.call(ListInstructionsVersions {}).await
    }

    pub async fn save_instructions(
        &self,
        text: String,
        base: Option<u32>,
    ) -> Result<InstructionsVersion, BeepError> {
        if let Some(mesh) = self.mesh().await? {
            return Ok(mesh
                .desk()
                .await?
                .save_instructions(&text, base, "")
                .await?);
        }
        self.call(SaveInstructions {
            text,
            base,
            note: String::new(),
        })
        .await
    }

    pub async fn read_agent_files(&self) -> Result<Vec<MachineAgentFiles>, BeepError> {
        if let Some(mesh) = self.mesh().await? {
            let newest = mesh.desk().await?.instructions().map(|value| value.text);
            let mut files = Vec::new();
            for machine in mesh
                .mesh
                .code_hosts()
                .into_iter()
                .filter(|machine| machine.online)
            {
                let id = machine.device.id.to_string();
                let client = mesh.client(&id).await?;
                files.push(agent_files::machine_files(
                    id,
                    machine.device.name,
                    agent_files::read(&client).await,
                    newest.as_deref(),
                ));
            }
            return Ok(files);
        }
        self.call(ReadAgentFiles {}).await
    }
}
