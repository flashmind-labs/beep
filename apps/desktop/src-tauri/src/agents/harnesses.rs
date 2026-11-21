//! The Coding agents page: the harnesses on each of the user's machines,
//! their settings, and installing or updating one there.

use crate::{
    beep::epoch_ms,
    error::Result,
    fleet::{
        Machines, agents as direct,
        projects::{FleetState, desk},
    },
    settings::credentials::plain,
};
use beep_proto::{
    AgentHarness, AgentHarnesses, HarnessEnv, HarnessSettings, HarnessSignIn,
    MachineHarnessSettings, SecretText,
    node::harness::{AutoCompact, HarnessDriver, ModelCompact},
};
use beep_proto::{AgentSlug, ModelId};
use serde::{Deserialize, Serialize};

/// One machine's harnesses as the page shows them.
#[derive(Debug, Serialize)]
pub struct HarnessesView {
    machine_id: String,
    machine_name: String,
    /// Milliseconds since the Unix epoch; null when never checked.
    checked_at: Option<u64>,
    harnesses: Vec<HarnessView>,
}

#[derive(Debug, Serialize)]
pub struct HarnessView {
    agent: String,
    installed: bool,
    runnable: bool,
    version: Option<String>,
    latest: Option<String>,
    program: Option<String>,
    home_var: Option<String>,
    home_default: Option<String>,
    sign_in: SignInView,
    install: Option<String>,
    settings: SettingsView,
}

/// How a harness signs in: `state` is `unknown`, `signed_in`,
/// `signed_out` or `beep_token`.
#[derive(Debug, Serialize)]
pub struct SignInView {
    state: &'static str,
    account: Option<String>,
    plan: Option<String>,
    last4: Option<String>,
}

/// The user's settings; `env` holds the names only, never the values.
#[derive(Debug, Serialize)]
pub struct SettingsView {
    enabled: bool,
    program: String,
    home: String,
    args: Vec<String>,
    env: Vec<String>,
}

/// The settings the page saves for one harness.
#[derive(Deserialize)]
pub struct SettingsInput {
    enabled: bool,
    program: String,
    home: String,
    args: Vec<String>,
    env: Vec<EnvInput>,
}

/// An env var the page saves: `value` null keeps the saved one.
#[derive(Deserialize)]
pub struct EnvInput {
    name: String,
    value: Option<String>,
}

/// One machine's settings for a harness, to say which fields differ.
#[derive(Debug, Serialize)]
pub struct MachineSettingsView {
    machine_id: String,
    machine_name: String,
    settings: SettingsView,
}

impl From<HarnessSettings> for SettingsView {
    fn from(settings: HarnessSettings) -> Self {
        Self {
            enabled: settings.enabled,
            program: settings.program,
            home: settings.home,
            args: settings.args,
            env: settings.env.into_iter().map(|var| var.name).collect(),
        }
    }
}

impl From<MachineHarnessSettings> for MachineSettingsView {
    fn from(machine: MachineHarnessSettings) -> Self {
        Self {
            machine_id: machine.machine_id,
            machine_name: machine.machine_name,
            settings: machine.settings.into(),
        }
    }
}

impl From<SettingsInput> for HarnessSettings {
    fn from(settings: SettingsInput) -> Self {
        Self {
            enabled: settings.enabled,
            program: settings.program,
            home: settings.home,
            args: settings.args,
            env: settings
                .env
                .into_iter()
                .map(|var| HarnessEnv {
                    name: var.name,
                    value: var.value.map(SecretText::new),
                })
                .collect(),
        }
    }
}

impl From<HarnessSignIn> for SignInView {
    fn from(sign_in: HarnessSignIn) -> Self {
        let none = |state| Self {
            state,
            account: None,
            plan: None,
            last4: None,
        };

        match sign_in {
            HarnessSignIn::Unknown {} => none("unknown"),
            HarnessSignIn::SignedOut {} => none("signed_out"),
            HarnessSignIn::SignedIn { account, plan } => Self {
                account,
                plan,
                ..none("signed_in")
            },
            HarnessSignIn::BeepToken { last4 } => Self {
                last4: Some(last4),
                ..none("beep_token")
            },
        }
    }
}

impl From<AgentHarness> for HarnessView {
    fn from(harness: AgentHarness) -> Self {
        Self {
            agent: harness.agent,
            installed: harness.installed,
            runnable: harness.runnable,
            version: harness.version,
            latest: harness.latest,
            program: harness.program,
            home_var: harness.home_var,
            home_default: harness.home_default,
            sign_in: harness.sign_in.into(),
            install: harness.install,
            settings: harness.settings.into(),
        }
    }
}

impl From<AgentHarnesses> for HarnessesView {
    fn from(list: AgentHarnesses) -> Self {
        Self {
            machine_id: list.machine_id,
            machine_name: list.machine_name,
            checked_at: list.checked_at.map(epoch_ms),
            harnesses: list.harnesses.into_iter().map(Into::into).collect(),
        }
    }
}

/// The harnesses on `machine_id`. `refresh` looks the newest versions up
/// again.
#[tauri::command]
pub async fn agent_harnesses(
    fleet: Machines<'_>,
    machine_id: String,
    refresh: bool,
) -> Result<Option<HarnessesView>> {
    let list = direct::harnesses(&fleet, fleet.machine(&machine_id)?, refresh)
        .await
        .map_err(plain)?;
    Ok(Some(list.into()))
}

/// Saves the settings of `agent` on `machine_id`.
#[tauri::command]
pub async fn harness_settings_save(
    fleet: FleetState<'_>,
    machine_id: String,
    agent: String,
    settings: SettingsInput,
) -> Result<()> {
    desk(&fleet)
        .await?
        .save_harness_settings(&machine_id, &agent, settings.into())
        .await?;
    Ok(())
}

/// Installs or updates `agent` on `machine_id`, and gives the harnesses
/// after.
#[tauri::command]
pub async fn agent_harness_update(
    fleet: Machines<'_>,
    machine_id: String,
    agent: String,
) -> Result<HarnessesView> {
    let updated = direct::update_harness(&fleet, fleet.machine(&machine_id)?, &agent)
        .await
        .map_err(plain)?;
    Ok(updated.into())
}

/// Try again: `machine_id` installs the ACP adapters its agents lack now;
/// how it goes comes with the machine's next report.
#[tauri::command]
pub async fn agent_adapters_install(fleet: Machines<'_>, machine_id: String) -> Result<()> {
    direct::install_adapters(&fleet, fleet.machine(&machine_id)?).await
}

/// The settings of `agent` on each of the user's machines.
#[tauri::command]
pub async fn harness_settings_all(
    fleet: FleetState<'_>,
    agent: String,
) -> Result<Option<Vec<MachineSettingsView>>> {
    let all = desk(&fleet).await?.harness_settings(&agent)?;
    Ok(Some(all.into_iter().map(Into::into).collect()))
}

/// Saves the settings of `agent`, as edited on `from_machine`, on every
/// machine of the user's; the binary path only with `include_program`.
/// Gives how many machines were saved.
#[tauri::command]
pub async fn harness_settings_save_all(
    fleet: FleetState<'_>,
    from_machine: String,
    agent: String,
    settings: SettingsInput,
    include_program: bool,
) -> Result<u32> {
    Ok(desk(&fleet)
        .await?
        .save_harness_settings_all(&from_machine, &agent, settings.into(), include_program)
        .await?)
}

/// When the nodes compact `agent`'s sessions; null is the default.
#[tauri::command]
pub async fn harness_compact(fleet: FleetState<'_>, agent: String) -> Result<Option<AutoCompact>> {
    Ok(desk(&fleet).await?.harness_compact(&agent)?)
}

/// Saves when the nodes compact `agent`'s sessions, on every machine.
#[tauri::command]
pub async fn harness_compact_save(
    fleet: FleetState<'_>,
    agent: String,
    compact: Option<AutoCompact>,
) -> Result<()> {
    desk(&fleet)
        .await?
        .save_harness_compact(&agent, compact)
        .await?;
    Ok(())
}

/// Model compaction overrides, shared by every machine.
#[tauri::command]
pub async fn harness_model_compacts(
    fleet: FleetState<'_>,
    agent: String,
) -> Result<Vec<ModelCompact>> {
    Ok(desk(&fleet).await?.harness_model_compacts(&agent)?)
}

#[tauri::command]
pub async fn harness_model_compacts_save(
    fleet: FleetState<'_>,
    agent: String,
    rules: Vec<ModelCompact>,
) -> Result<()> {
    desk(&fleet)
        .await?
        .save_harness_model_compacts(&agent, rules)
        .await?;
    Ok(())
}

/// How the nodes drive `agent`, on every machine.
#[tauri::command]
pub async fn harness_driver(fleet: FleetState<'_>, agent: String) -> Result<HarnessDriver> {
    Ok(desk(&fleet).await?.harness_driver(&agent)?)
}

#[tauri::command]
pub async fn harness_driver_save(
    fleet: FleetState<'_>,
    agent: String,
    driver: HarnessDriver,
) -> Result<()> {
    Ok(desk(&fleet)
        .await?
        .save_harness_driver(&agent, driver)
        .await?)
}

/// The harnesses whose sessions may run in `project`, on every machine; `None` is any.
#[tauri::command]
pub async fn project_harnesses(
    fleet: FleetState<'_>,
    project: String,
) -> Result<Option<Vec<AgentSlug>>> {
    Ok(desk(&fleet).await?.project_harnesses(&project)?)
}

#[tauri::command]
pub async fn project_harnesses_save(
    fleet: FleetState<'_>,
    project: String,
    harnesses: Option<Vec<AgentSlug>>,
) -> Result<()> {
    Ok(desk(&fleet)
        .await?
        .save_project_harnesses(&project, harnesses)
        .await?)
}

/// Default model for new sessions on every machine.
#[tauri::command]
pub async fn harness_default_model(
    fleet: FleetState<'_>,
    agent: String,
) -> Result<Option<ModelId>> {
    Ok(desk(&fleet).await?.harness_default_model(&agent)?)
}

#[tauri::command]
pub async fn harness_default_model_save(
    fleet: FleetState<'_>,
    agent: String,
    model: Option<ModelId>,
) -> Result<()> {
    Ok(desk(&fleet)
        .await?
        .save_harness_default_model(&agent, model)
        .await?)
}
