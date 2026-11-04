//! The user's settings for every machine: coding agent harnesses, flsh, and the saved text of
//! their instructions. They are the same on every machine, so they are one value each in the
//! mesh (`harness/<agent>`, `flsh`, `instructions`), which the nodes read.

use crate::{
    desk::Desk,
    error::{FleetError, Result},
    records::{InstructionsRecord, SavedVersion, now_ms, read, time, write},
};
use beep_mesh::Key;
use beep_proto::{
    ApiError, FlshSettings, HarnessEnv, HarnessSettings, InstructionsVersion,
    MachineHarnessSettings, check_agent, check_settings, merged_env, node::harness::HarnessConfig,
};

/// The longest instructions, in bytes.
const MAX_TEXT: usize = 16 * 1024;
const MAX_NOTE: usize = 200;
/// The most versions listed.
const LISTED_VERSIONS: usize = 50;

fn settings_of(config: Option<&HarnessConfig>) -> HarnessSettings {
    match config {
        None => HarnessSettings {
            enabled: true,
            program: String::new(),
            home: String::new(),
            args: Vec::new(),
            env: Vec::new(),
        },
        Some(config) => HarnessSettings {
            enabled: config.enabled,
            program: config.program.clone().unwrap_or_default(),
            home: config.home.clone().unwrap_or_default(),
            args: config.args.clone(),
            // Never the values.
            env: config
                .env
                .iter()
                .map(|var| HarnessEnv {
                    name: var.name.clone(),
                    value: None,
                })
                .collect(),
        },
    }
}

fn version(index: usize, saved: &SavedVersion) -> InstructionsVersion {
    InstructionsVersion {
        version: u32::try_from(index + 1).unwrap_or(u32::MAX),
        text: saved.text.clone(),
        note: saved.note.clone(),
        saved_at: time(saved.saved_ms),
    }
}

impl Desk {
    fn harness_key(agent: &str) -> Key {
        Key::Harness(agent.to_owned())
    }

    /// Each machine's settings for `agent`: the same on all of them.
    pub fn harness_settings(&self, agent: &str) -> Result<Vec<MachineHarnessSettings>> {
        check_agent(agent)?;
        let config = read::<HarnessConfig>(&self.mesh, &Self::harness_key(agent));
        Ok(self
            .mesh
            .devices()
            .into_iter()
            .map(|status| MachineHarnessSettings {
                machine_id: status.device.id.to_string(),
                machine_name: status.device.name,
                settings: settings_of(config.as_ref()),
            })
            .collect())
    }

    /// Saves the settings for `agent` on every machine. A variable with no value keeps the saved
    /// one. Returns how many machines have them.
    pub async fn save_harness_settings(
        &self,
        machine: &str,
        agent: &str,
        settings: HarnessSettings,
    ) -> Result<u32> {
        let machines = self.mesh.devices();
        if !machines
            .iter()
            .any(|status| status.device.id.to_string() == machine)
        {
            return Err(FleetError::NotFound);
        }
        check_agent(agent)?;
        let settings = check_settings(settings)?;
        let key = Self::harness_key(agent);
        let kept = read::<HarnessConfig>(&self.mesh, &key)
            .map(|config| config.env)
            .unwrap_or_default();
        let blank = |text: String| Some(text).filter(|text| !text.is_empty());
        let config = HarnessConfig {
            agent: agent.to_owned(),
            enabled: settings.enabled,
            program: blank(settings.program),
            home: blank(settings.home),
            args: settings.args,
            env: merged_env(kept, settings.env),
        };
        write(&self.mesh, key, Some(&config)).await?;
        Ok(u32::try_from(machines.len()).unwrap_or(u32::MAX))
    }

    /// flsh's settings as the user set them.
    pub fn flsh(&self) -> FlshSettings {
        read::<FlshSettings>(&self.mesh, &Key::Flsh).unwrap_or_default()
    }

    pub async fn save_flsh(&self, settings: &FlshSettings) -> Result<()> {
        settings
            .check()
            .map_err(|e| FleetError::refused(e.to_string()))?;
        write(&self.mesh, Key::Flsh, Some(settings)).await
    }

    fn saved_versions(&self) -> Vec<SavedVersion> {
        read::<InstructionsRecord>(&self.mesh, &Key::Instructions)
            .map(|record| record.versions)
            .unwrap_or_default()
    }

    /// The newest saved version of the user's instructions.
    pub fn instructions(&self) -> Option<InstructionsVersion> {
        let versions = self.saved_versions();
        let last = versions.last()?;
        Some(version(versions.len() - 1, last))
    }

    /// Every saved version, the newest first.
    pub fn instructions_versions(&self) -> Vec<InstructionsVersion> {
        let versions = self.saved_versions();
        versions
            .iter()
            .enumerate()
            .rev()
            .take(LISTED_VERSIONS)
            .map(|(index, saved)| version(index, saved))
            .collect()
    }

    /// Saves `text` as the next version, unless `base` is not the newest.
    pub async fn save_instructions(
        &self,
        text: &str,
        base: Option<u32>,
        note: &str,
    ) -> Result<InstructionsVersion> {
        if text.len() > MAX_TEXT {
            return Err(FleetError::refused("Instructions can be at most 16 KB."));
        }
        let note = note.trim();
        if note.len() > MAX_NOTE || note.chars().any(char::is_control) {
            return Err(FleetError::refused(format!(
                "A note can be at most {MAX_NOTE} characters on one line."
            )));
        }
        let _write = crate::records::WRITES.lock().await;
        let mut versions = self.saved_versions();
        let newest = u32::try_from(versions.len()).unwrap_or(u32::MAX);
        if base.unwrap_or(0) != newest {
            return Err(ApiError::Conflict {
                message: format!("Version {newest} was saved since. Load it, then edit again."),
            }
            .into());
        }
        let saved = SavedVersion {
            text: text.to_owned(),
            note: note.to_owned(),
            saved_ms: now_ms(),
        };
        let made = version(versions.len(), &saved);
        versions.push(saved);
        write(
            &self.mesh,
            Key::Instructions,
            Some(&InstructionsRecord { versions }),
        )
        .await?;
        Ok(made)
    }
}
