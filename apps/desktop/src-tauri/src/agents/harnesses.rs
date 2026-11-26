//! The Coding agents page: the harnesses on each of the user's machines,
//! their settings, and installing or updating one there.

use crate::{
    error::{Result, plain},
    fleet::{
        Machines, agents as direct,
        node_actor::{FAST_JOB_WAIT, JobKind},
        projects::{FleetState, desk},
    },
};
use beep_proto::{
    AgentHarnesses, AgentSlug, HarnessSettings, MachineHarnessSettings, ModelId,
    node::harness::{AutoCompact, HarnessDriver, ModelCompact},
};
use tauri::AppHandle;

/// The harnesses on `machine_id`. `refresh` looks the newest versions up
/// again.
#[tauri::command]
pub async fn agent_harnesses(
    fleet: Machines<'_>,
    machine_id: String,
    refresh: bool,
) -> Result<Option<AgentHarnesses>> {
    let list = direct::harnesses(&fleet, fleet.machine(&machine_id)?, refresh)
        .await
        .map_err(plain)?;
    Ok(Some(list))
}

/// Saves the settings of `agent` on `machine_id`.
#[tauri::command]
pub async fn harness_settings_save(
    app: AppHandle,
    fleet: FleetState<'_>,
    machines: Machines<'_>,
    machine_id: String,
    agent: String,
    settings: HarnessSettings,
) -> Result<()> {
    desk(&fleet)
        .await?
        .save_harness_settings(&machine_id, &agent, settings)
        .await?;
    if let Some(machine) = machines.member(&machine_id) {
        direct::models_changed(&app, &machines, machine);
    }
    Ok(())
}

/// Installs or updates `agent` on `machine_id`, and gives the harnesses
/// after.
#[tauri::command]
pub async fn agent_harness_update(
    app: AppHandle,
    fleet: Machines<'_>,
    machine_id: String,
    agent: String,
) -> Result<AgentHarnesses> {
    let machine = fleet.machine(&machine_id)?;
    let list = direct::update_harness(&fleet, machine, &agent)
        .await
        .map_err(plain)?;
    direct::models_changed(&app, &fleet, machine);
    Ok(list)
}

/// Try again: `machine_id` installs the ACP adapters its agents lack now;
/// how it goes comes with the machine's next report.
#[tauri::command]
pub async fn agent_adapters_install(
    app: AppHandle,
    fleet: Machines<'_>,
    machine_id: String,
) -> Result<()> {
    let machine = fleet.machine(&machine_id)?;
    let fleet_arc = fleet.inner().clone();
    fleet
        .node_ops()
        .submit(
            Some(app),
            fleet_arc,
            machine,
            JobKind::InstallAdapters,
            FAST_JOB_WAIT,
        )
        .await
        .map(|_| ())
}

/// The settings of `agent` on each of the user's machines.
#[tauri::command]
pub async fn harness_settings_all(
    fleet: FleetState<'_>,
    agent: String,
) -> Result<Option<Vec<MachineHarnessSettings>>> {
    Ok(Some(desk(&fleet).await?.harness_settings(&agent)?))
}

/// Saves the settings of `agent`, as edited on `from_machine`, on every
/// machine of the user's; the binary path only with `include_program`.
/// Gives how many machines were saved.
#[tauri::command]
pub async fn harness_settings_save_all(
    fleet: FleetState<'_>,
    from_machine: String,
    agent: String,
    settings: HarnessSettings,
    include_program: bool,
) -> Result<u32> {
    Ok(desk(&fleet)
        .await?
        .save_harness_settings_all(&from_machine, &agent, settings, include_program)
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
