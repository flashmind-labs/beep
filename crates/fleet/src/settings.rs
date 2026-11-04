//! The user's settings for every machine: coding agent harnesses, flsh, and the saved text of
//! their instructions. They are the same on every machine, so they are one value each in the
//! mesh (`harness/<agent>`, `flsh`, `instructions`), which the nodes read.

use crate::{
    desk::Desk,
    error::{FleetError, Result},
    records::{SavedVersion, WRITES, now_ms, read, scan, time, write},
};
use beep_mesh::Key;
use beep_proto::{
    ApiError, FlshSettings, HarnessEnv, HarnessSettings, InstructionsVersion,
    MachineHarnessSettings, check_agent, check_settings, merged_env, node::harness::HarnessConfig,
};
use iroh::EndpointId;
use uuid::Uuid;

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

    /// Where a machine's own program path and home folder for `agent` are kept: what is a path on
    /// one machine must not reach another.
    fn harness_device_key(agent: &str, machine: EndpointId) -> Key {
        Key::Harness(format!("{agent}/{machine}"))
    }

    /// Each machine's settings for `agent`: the same on all of them, but for its own program path
    /// and home folder.
    pub fn harness_settings(&self, agent: &str) -> Result<Vec<MachineHarnessSettings>> {
        check_agent(agent)?;
        let shared = read::<HarnessConfig>(&self.mesh, &Self::harness_key(agent));
        Ok(self
            .mesh
            .devices()
            .into_iter()
            .map(|status| {
                let own = read::<HarnessConfig>(
                    &self.mesh,
                    &Self::harness_device_key(agent, status.device.id),
                );
                let mut settings = settings_of(shared.as_ref());
                settings.program = own
                    .as_ref()
                    .and_then(|own| own.program.clone())
                    .unwrap_or_default();
                settings.home = own.and_then(|own| own.home).unwrap_or_default();
                MachineHarnessSettings {
                    machine_id: status.device.id.to_string(),
                    machine_name: status.device.name,
                    settings,
                }
            })
            .collect())
    }

    /// Saves the settings for `agent`, as edited on `machine`: the part every machine shares, and
    /// that machine's own program path and home folder. A variable with no value keeps the saved
    /// one. Returns how many machines have the shared part.
    pub async fn save_harness_settings(
        &self,
        machine: &str,
        agent: &str,
        settings: HarnessSettings,
    ) -> Result<u32> {
        let (id, devices) = self.known_machine(machine)?;
        let paths = Some((id, settings.program.clone(), settings.home.clone()));
        self.save_harness(agent, settings, paths.into_iter().collect())
            .await?;
        Ok(devices)
    }

    /// Saves `settings` for every machine. The program path and home folder, which are paths on
    /// the machine it was edited on, go to all of them only with `include_program`.
    pub async fn save_harness_settings_all(
        &self,
        from_machine: &str,
        agent: &str,
        settings: HarnessSettings,
        include_program: bool,
    ) -> Result<u32> {
        let (_, devices) = self.known_machine(from_machine)?;
        let paths = match include_program {
            true => self
                .mesh
                .devices()
                .into_iter()
                .map(|status| {
                    (
                        status.device.id,
                        settings.program.clone(),
                        settings.home.clone(),
                    )
                })
                .collect(),
            false => Vec::new(),
        };
        self.save_harness(agent, settings, paths).await?;
        Ok(devices)
    }

    fn known_machine(&self, machine: &str) -> Result<(EndpointId, u32)> {
        let machines = self.mesh.devices();
        let id = machines
            .iter()
            .map(|status| status.device.id)
            .find(|id| id.to_string() == machine)
            .ok_or(FleetError::NotFound)?;
        Ok((id, u32::try_from(machines.len()).unwrap_or(u32::MAX)))
    }

    async fn save_harness(
        &self,
        agent: &str,
        settings: HarnessSettings,
        paths: Vec<(EndpointId, String, String)>,
    ) -> Result<()> {
        check_agent(agent)?;
        let settings = check_settings(settings)?;
        let key = Self::harness_key(agent);
        let kept = read::<HarnessConfig>(&self.mesh, &key)
            .map(|config| config.env)
            .unwrap_or_default();
        let config = HarnessConfig {
            agent: agent.to_owned(),
            enabled: settings.enabled,
            program: None,
            home: None,
            args: settings.args,
            env: merged_env(kept, settings.env),
        };
        write(&self.mesh, key, Some(&config)).await?;
        let blank = |text: String| Some(text).filter(|text| !text.is_empty());
        for (machine, program, home) in paths {
            let own = HarnessConfig {
                program: blank(program),
                home: blank(home),
                ..HarnessConfig::new(agent)
            };
            let key = Self::harness_device_key(agent, machine);
            match (&own.program, &own.home) {
                (None, None) => {
                    if self.mesh.get(&key).is_some() {
                        self.mesh.set(key, None).await?;
                    }
                }
                _ => write(&self.mesh, key, Some(&own)).await?,
            }
        }
        Ok(())
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

    /// Every saved version with its key, oldest first.
    fn saved_versions(&self) -> Vec<(Key, SavedVersion)> {
        let mut versions: Vec<(Key, SavedVersion)> = scan_versions(&self.mesh);
        versions.sort_by(|a, b| (a.1.saved_ms, &a.0).cmp(&(b.1.saved_ms, &b.0)));
        versions
    }

    /// The newest saved version of the user's instructions.
    pub fn instructions(&self) -> Option<InstructionsVersion> {
        let versions = self.saved_versions();
        let (_, last) = versions.last()?;
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
            .map(|(index, (_, saved))| version(index, saved))
            .collect()
    }

    /// Saves `text` as the next version, unless `base` is not the newest. Each version is its own
    /// key, so a version another desktop saves at the same moment is kept beside this one.
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
        let _write = WRITES.lock().await;
        let held = self.saved_versions().len();
        let newest = u32::try_from(held).unwrap_or(u32::MAX);
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
        let key = Key::Group(format!(
            "{VERSION_PREFIX}{}.{}",
            saved.saved_ms,
            Uuid::new_v4()
        ));
        write(&self.mesh, key, Some(&saved)).await?;
        Ok(version(held, &saved))
    }
}

/// What the id of a version's key starts with.
pub(crate) const VERSION_PREFIX: &str = "instructions.";

fn scan_versions(mesh: &beep_mesh::Mesh) -> Vec<(Key, SavedVersion)> {
    let prefix = format!("group/{VERSION_PREFIX}");
    scan::<SavedVersion>(mesh, &prefix)
}
