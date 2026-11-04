//! The Coding agents page: the harnesses on each of the user's machines,
//! their settings, and installing or updating one there.

use crate::{
    beep::{Beep, epoch_ms},
    error::Result,
    projects::unknown_op,
    settings::credentials::plain,
};
use beep_proto::{
    AgentHarness, AgentHarnesses, HarnessEnv, HarnessSettings, HarnessSignIn, ListAgentHarnesses,
    ListHarnessSettings, MachineHarnessSettings, SaveHarnessSettings, SaveHarnessSettingsAll,
    SecretText, UpdateAgentHarness,
};
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Duration};
use tauri::State;

type Service<'a> = State<'a, Arc<Beep>>;

/// How long the server may take to ask the machine: it runs each
/// harness's version and sign-in commands there.
const LIST_LIMIT: Duration = Duration::from_secs(60);
/// How long an install or update may take on the machine.
const UPDATE_LIMIT: Duration = Duration::from_secs(12 * 60);

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

/// The harnesses on `machine_id`; `None` from a server too old to say.
/// `refresh` looks the newest versions up again.
#[tauri::command]
pub async fn agent_harnesses(
    beep: Service<'_>,
    machine_id: String,
    refresh: bool,
) -> Result<Option<HarnessesView>> {
    let request = ListAgentHarnesses {
        machine_id,
        refresh,
    };
    match beep.call_within(request, LIST_LIMIT).await {
        Ok(list) => Ok(Some(list.into())),
        Err(e) if unknown_op(&e) => Ok(None),
        Err(e) => Err(plain(e)),
    }
}

/// Saves the settings of `agent` on `machine_id`.
#[tauri::command]
pub async fn harness_settings_save(
    beep: Service<'_>,
    machine_id: String,
    agent: String,
    settings: SettingsInput,
) -> Result<()> {
    beep.call(SaveHarnessSettings {
        machine_id,
        agent,
        settings: settings.into(),
    })
    .await
    .map_err(plain)
}

/// Installs or updates `agent` on `machine_id`, and gives the harnesses
/// after.
#[tauri::command]
pub async fn agent_harness_update(
    beep: Service<'_>,
    machine_id: String,
    agent: String,
) -> Result<HarnessesView> {
    let updated = beep
        .call_within(UpdateAgentHarness { machine_id, agent }, UPDATE_LIMIT)
        .await
        .map_err(plain)?;
    Ok(updated.into())
}

/// The settings of `agent` on each of the user's machines; `None` from a
/// server too old to say.
#[tauri::command]
pub async fn harness_settings_all(
    beep: Service<'_>,
    agent: String,
) -> Result<Option<Vec<MachineSettingsView>>> {
    match beep.call(ListHarnessSettings { agent }).await {
        Ok(all) => Ok(Some(all.into_iter().map(Into::into).collect())),
        Err(e) if unknown_op(&e) => Ok(None),
        Err(e) => Err(plain(e)),
    }
}

/// Saves the settings of `agent`, as edited on `from_machine`, on every
/// machine of the user's; the binary path only with `include_program`.
/// Gives how many machines were saved.
#[tauri::command]
pub async fn harness_settings_save_all(
    beep: Service<'_>,
    from_machine: String,
    agent: String,
    settings: SettingsInput,
    include_program: bool,
) -> Result<u32> {
    beep.call(SaveHarnessSettingsAll {
        from_machine,
        agent,
        settings: settings.into(),
        include_program,
    })
    .await
    .map_err(plain)
}
