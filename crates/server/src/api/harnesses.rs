//! Saved coding-agent settings for the caller's machines.

use super::Reply;
use crate::{
    account::credentials::CredentialKey,
    account::user::User,
    api::router::Cx,
    protocol::{node::EnvSecret, *},
    store::{HarnessRow, SaveHarness},
};
use anyhow::Context;

pub(super) async fn save_harness_settings(
    cx: Cx,
    SaveHarnessSettings {
        machine_id,
        agent,
        settings,
    }: SaveHarnessSettings,
) -> Reply<()> {
    let nodes = cx.user.nodes().await?;
    if !nodes.iter().any(|node| node.endpoint == machine_id) {
        return Err(ApiError::NotFound {});
    }
    check_agent(&agent)?;
    let settings = check_settings(settings)?;
    let key = &cx.app.credential_key;
    let env = env_to_save(&cx.user, key, &machine_id, &agent, settings.env).await?;
    cx.user
        .save_harness_settings(
            key,
            SaveHarness {
                machine_id: &machine_id,
                agent: &agent,
                enabled: settings.enabled,
                program: &settings.program,
                home: &settings.home,
                args: &settings.args,
                env: &env,
            },
        )
        .await?;
    Ok(())
}

pub(super) async fn list_harness_settings(
    cx: Cx,
    ListHarnessSettings { agent }: ListHarnessSettings,
) -> Reply<Vec<MachineHarnessSettings>> {
    check_agent(&agent)?;
    let rows = cx.user.harness_settings_of(&agent).await?;
    Ok(cx
        .user
        .nodes()
        .await?
        .into_iter()
        .map(|node| {
            let row = rows.iter().find(|row| row.machine_id == node.endpoint);
            MachineHarnessSettings {
                settings: settings_of(row),
                machine_id: node.endpoint,
                machine_name: node.name,
            }
        })
        .collect())
}

pub(super) async fn save_harness_settings_all(
    cx: Cx,
    SaveHarnessSettingsAll {
        from_machine,
        agent,
        settings,
        include_program,
    }: SaveHarnessSettingsAll,
) -> Reply<u32> {
    let machines: Vec<String> = cx
        .user
        .nodes()
        .await?
        .into_iter()
        .map(|node| node.endpoint)
        .collect();
    if !machines.contains(&from_machine) {
        return Err(ApiError::NotFound {});
    }
    check_agent(&agent)?;
    let settings = check_settings(settings)?;
    let everywhere = Everywhere {
        machines: &machines,
        from_machine: &from_machine,
        agent: &agent,
        include_program,
    };
    let saved = save_everywhere(&cx.user, &cx.app.credential_key, &everywhere, settings).await?;
    Ok(u32::try_from(saved.len()).context("too many machines")?)
}

/// Where `SaveHarnessSettingsAll` saves, and what it copies.
struct Everywhere<'a> {
    machines: &'a [String],
    from_machine: &'a str,
    agent: &'a str,
    include_program: bool,
}

/// What one machine keeps of its own when settings are saved to all of them.
#[derive(Debug, PartialEq)]
struct Kept {
    machine_id: String,
    enabled: bool,
    program: String,
}

fn kept_per_machine(
    everywhere: &Everywhere<'_>,
    rows: &[HarnessRow],
    edited: &HarnessSettings,
) -> Vec<Kept> {
    everywhere
        .machines
        .iter()
        .map(|machine| {
            let row = rows.iter().find(|row| &row.machine_id == machine);
            let source = machine == everywhere.from_machine;
            Kept {
                machine_id: machine.clone(),
                enabled: if source {
                    edited.enabled
                } else {
                    row.is_none_or(|row| row.enabled)
                },
                program: if source || everywhere.include_program {
                    edited.program.clone()
                } else {
                    row.map(|row| row.program.clone()).unwrap_or_default()
                },
            }
        })
        .collect()
}

/// Saves `settings` on every machine, sealing env vars per machine; gives the machines saved.
async fn save_everywhere(
    user: &User,
    key: &CredentialKey,
    everywhere: &Everywhere<'_>,
    settings: HarnessSettings,
) -> anyhow::Result<Vec<String>> {
    let agent = everywhere.agent;
    let rows = user.harness_settings_of(agent).await?;
    let kept = kept_per_machine(everywhere, &rows, &settings);
    let env = env_to_save(user, key, everywhere.from_machine, agent, settings.env).await?;
    let mut saved = Vec::with_capacity(kept.len());
    for machine in kept {
        user.save_harness_settings(
            key,
            SaveHarness {
                machine_id: &machine.machine_id,
                agent,
                enabled: machine.enabled,
                program: &machine.program,
                home: &settings.home,
                args: &settings.args,
                env: &env,
            },
        )
        .await
        .with_context(|| {
            format!(
                "the {agent} settings for {} were not saved",
                machine.machine_id
            )
        })?;
        saved.push(machine.machine_id);
    }
    Ok(saved)
}

pub(super) async fn get_flsh_settings(
    cx: Cx,
    GetFlshSettings {}: GetFlshSettings,
) -> Reply<FlshSettings> {
    Ok(cx.user.flsh_settings().await?.unwrap_or_default())
}

pub(super) async fn save_flsh_settings(
    cx: Cx,
    SaveFlshSettings { settings }: SaveFlshSettings,
) -> Reply<()> {
    settings
        .check()
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    cx.user.save_flsh_settings(&settings).await?;
    Ok(())
}

/// A harness's saved settings as the apps see them, env var names without values.
fn settings_of(row: Option<&HarnessRow>) -> HarnessSettings {
    row.map_or_else(
        || HarnessSettings {
            enabled: true,
            program: String::new(),
            home: String::new(),
            args: Vec::new(),
            env: Vec::new(),
        },
        |row| HarnessSettings {
            enabled: row.enabled,
            program: row.program.clone(),
            home: row.home.clone(),
            args: row.args.clone(),
            // Never the values.
            env: row
                .env_names
                .iter()
                .map(|name| HarnessEnv {
                    name: name.clone(),
                    value: None,
                })
                .collect(),
        },
    )
}

/// The env vars to save; saved vars that do not open stop the save rather than being dropped.
async fn env_to_save(
    user: &User,
    key: &CredentialKey,
    machine_id: &str,
    agent: &str,
    wanted: Vec<HarnessEnv>,
) -> anyhow::Result<Vec<EnvSecret>> {
    let kept = if wanted.iter().any(|var| var.value.is_none()) {
        user.harness_env(key, machine_id, agent)
            .await
            .with_context(|| format!("the saved {agent} env vars do not open"))?
    } else {
        Vec::new()
    };
    Ok(merged_env(kept, wanted))
}

#[cfg(test)]
#[path = "../tests/api_harnesses.rs"]
mod tests;
