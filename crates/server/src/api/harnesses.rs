//! The Coding agents page: the harnesses on each of the caller's machines,
//! with what the node finds of each and the caller's settings for it, and
//! installing or updating one there.

use super::{Reply, own_nodes};
use crate::{
    credentials::CredentialKey,
    nodes::{Online, agent_name},
    protocol::{
        node::{
            EnvSecret,
            harness::{HarnessInfo, HarnessReport, harness},
        },
        *,
    },
    router::Cx,
    store::{AgentTokenRow, HarnessRow, SaveHarness},
    user::User,
};
use anyhow::Context;
use std::{
    collections::HashSet,
    time::{Duration, UNIX_EPOCH},
};

/// The longest program path or home folder taken.
const MAX_PATH: usize = 1024;
/// The most launch arguments, and the longest one.
const MAX_ARGS: usize = 32;
const MAX_ARG: usize = 1024;
/// The most env vars, the longest name and the longest value.
const MAX_ENV: usize = 32;
const MAX_ENV_NAME: usize = 128;
const MAX_ENV_VALUE: usize = 8 * 1024;
/// The longest agent slug, for one a machine's `node.toml` adds.
const MAX_AGENT: usize = 32;

pub(super) async fn list_agent_harnesses(
    cx: Cx,
    ListAgentHarnesses {
        machine_id,
        refresh,
    }: ListAgentHarnesses,
) -> Reply<AgentHarnesses> {
    let machine = online_machine(&cx, &machine_id).await?;
    let report = machine
        .online
        .harnesses(refresh)
        .await
        .map_err(|e| ApiError::bad_request(format!("{}: {e:#}", machine.name)))?
        .ok_or_else(|| too_old(&machine.name))?;
    view(&cx, machine, report).await
}

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
    checked_agent(&agent)?;
    let settings = checked(settings)?;
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
    cx.app
        .nodes
        .agent_settings_changed(cx.user.id(), &machine_id);
    Ok(())
}

pub(super) async fn list_harness_settings(
    cx: Cx,
    ListHarnessSettings { agent }: ListHarnessSettings,
) -> Reply<Vec<MachineHarnessSettings>> {
    checked_agent(&agent)?;
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
    checked_agent(&agent)?;
    let settings = checked(settings)?;
    let everywhere = Everywhere {
        machines: &machines,
        from_machine: &from_machine,
        agent: &agent,
        include_program,
    };
    let saved = save_everywhere(&cx.user, &cx.app.credential_key, &everywhere, settings).await?;
    for machine in &saved {
        cx.app.nodes.agent_settings_changed(cx.user.id(), machine);
    }
    Ok(u32::try_from(saved.len()).context("too many machines")?)
}

/// Where `SaveHarnessSettingsAll` saves, and what it copies.
struct Everywhere<'a> {
    machines: &'a [String],
    from_machine: &'a str,
    agent: &'a str,
    include_program: bool,
}

/// What one machine keeps of its own when settings are saved to all of
/// them: whether the harness is on there, and its binary path unless that
/// is copied too or it is the machine the settings were edited on.
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

/// Saves checked `settings` for the harness on every machine, each env
/// var sealed for its own machine; a var with no value takes the one saved
/// on the source machine. Gives the machines saved.
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
    cx.app.nodes.agent_settings_changed_everywhere(cx.user.id());
    Ok(())
}

pub(super) async fn update_agent_harness(
    cx: Cx,
    UpdateAgentHarness { machine_id, agent }: UpdateAgentHarness,
) -> Reply<AgentHarnesses> {
    checked_agent(&agent)?;
    let machine = online_machine(&cx, &machine_id).await?;
    let report = machine
        .online
        .update_harness(&agent)
        .await
        .map_err(|e| {
            ApiError::bad_request(format!(
                "Could not update {} on {}: {e:#}",
                agent_name(&agent),
                machine.name
            ))
        })?
        .ok_or_else(|| too_old(&machine.name))?;
    view(&cx, machine, report).await
}

/// One of the caller's machines, online now.
struct Machine {
    id: String,
    name: String,
    online: Online,
}

async fn online_machine(cx: &Cx, machine_id: &str) -> Reply<Machine> {
    let node = cx
        .user
        .nodes()
        .await?
        .into_iter()
        .find(|node| node.endpoint == machine_id)
        .ok_or(ApiError::NotFound {})?;
    let online = own_nodes::online(cx, machine_id).ok_or(ApiError::NotFound {})?;
    Ok(Machine {
        id: node.endpoint,
        name: node.name,
        online,
    })
}

fn too_old(machine: &str) -> ApiError {
    ApiError::bad_request(format!(
        "Update beep-node on {machine} to manage its coding agents here."
    ))
}

/// The page's answer: `report` with the caller's settings and tokens.
async fn view(cx: &Cx, machine: Machine, report: HarnessReport) -> Reply<AgentHarnesses> {
    let rows = cx.user.harness_settings(&machine.id).await?;
    let tokens = cx.user.agent_tokens(&cx.app.credential_key).await?;
    Ok(AgentHarnesses {
        machine_id: machine.id,
        machine_name: machine.name,
        checked_at: (report.checked_at > 0)
            .then(|| UNIX_EPOCH + Duration::from_secs(report.checked_at)),
        harnesses: report
            .harnesses
            .into_iter()
            .map(|info| harness_view(info, &rows, &tokens))
            .collect(),
    })
}

/// One harness as the page shows it.
fn harness_view(info: HarnessInfo, rows: &[HarnessRow], tokens: &[AgentTokenRow]) -> AgentHarness {
    let known = harness(&info.agent);
    let token = tokens
        .iter()
        .find(|token| token.agent.slug() == info.agent && !token.unreadable);
    let sign_in = match (token, info.signed_in) {
        (Some(token), _) => HarnessSignIn::BeepToken {
            last4: token.last4.clone(),
        },
        (None, Some(true)) => HarnessSignIn::SignedIn {
            account: info.account,
            plan: info.plan,
        },
        (None, Some(false)) => HarnessSignIn::SignedOut {},
        (None, None) => HarnessSignIn::Unknown {},
    };
    let settings = settings_of(rows.iter().find(|row| row.agent == info.agent));
    AgentHarness {
        agent: info.agent,
        installed: info.installed,
        runnable: info.runnable,
        version: info.version,
        latest: info.latest,
        program: info.program,
        home_var: known.map(|known| known.home_var.to_owned()),
        home_default: known.map(|known| known.home_default.to_owned()),
        sign_in,
        install: info.install,
        settings,
    }
}

/// A harness's saved settings as the apps see them, the env var names
/// without their values; the defaults when none are saved.
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

/// A slug Beep knows, or one a machine's `node.toml` could add.
fn checked_agent(agent: &str) -> Reply<()> {
    let plain = !agent.is_empty()
        && agent.len() <= MAX_AGENT
        && agent
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_');
    if harness(agent).is_some() || plain {
        Ok(())
    } else {
        Err(ApiError::bad_request(format!(
            "{agent:?} is not a coding agent."
        )))
    }
}

/// `settings` trimmed, or why they cannot be saved.
fn checked(settings: HarnessSettings) -> Reply<HarnessSettings> {
    let line = |what: &str, text: &str, max: usize| -> Reply<String> {
        let text = text.trim();
        if text.len() > max {
            return Err(ApiError::bad_request(format!("The {what} is too long.")));
        }
        if text.chars().any(char::is_control) {
            return Err(ApiError::bad_request(format!(
                "The {what} cannot hold a line break or control character."
            )));
        }
        Ok(text.to_owned())
    };
    let program = line("binary path", &settings.program, MAX_PATH)?;
    let home = line("home folder", &settings.home, MAX_PATH)?;
    if settings.args.len() > MAX_ARGS {
        return Err(ApiError::bad_request(format!(
            "At most {MAX_ARGS} launch arguments."
        )));
    }
    let args = settings
        .args
        .iter()
        .map(|arg| line("launch argument", arg, MAX_ARG))
        .filter(|arg| !matches!(arg, Ok(arg) if arg.is_empty()))
        .collect::<Reply<Vec<String>>>()?;
    if settings.env.len() > MAX_ENV {
        return Err(ApiError::bad_request(format!(
            "At most {MAX_ENV} environment variables."
        )));
    }
    let mut seen = HashSet::new();
    let mut env = Vec::new();
    for var in settings.env {
        let name = var.name.trim().to_owned();
        if name.is_empty() && var.value.is_none() {
            continue;
        }
        let valid = name.len() <= MAX_ENV_NAME
            && name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !valid {
            return Err(ApiError::bad_request(format!(
                "{name:?} is not an environment variable name."
            )));
        }
        if !seen.insert(name.clone()) {
            return Err(ApiError::bad_request(format!("{name} is set twice.")));
        }
        if let Some(value) = &var.value
            && (value.expose().len() > MAX_ENV_VALUE || value.expose().contains('\0'))
        {
            return Err(ApiError::bad_request(format!(
                "The value of {name} is too long or not text."
            )));
        }
        env.push(HarnessEnv {
            name,
            value: var.value,
        });
    }
    Ok(HarnessSettings {
        enabled: settings.enabled,
        program,
        home,
        args,
        env,
    })
}

/// The env vars to save for `agent` on `machine_id`. A var sent without a
/// value keeps the saved one, so saved vars that do not open stop the save
/// rather than being dropped.
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

/// The env vars to save: each of `wanted` with its new value, or with the
/// value `kept` had for that name; one with neither is dropped.
fn merged_env(kept: Vec<EnvSecret>, wanted: Vec<HarnessEnv>) -> Vec<EnvSecret> {
    wanted
        .into_iter()
        .filter_map(|var| {
            let value = var.value.or_else(|| {
                kept.iter()
                    .find(|old| old.name == var.name)
                    .map(|old| old.value.clone())
            })?;
            Some(EnvSecret {
                name: var.name,
                value,
            })
        })
        .collect()
}

/// Whether the caller turned `agent` off on `machine_id`.
pub(super) fn is_off(off: &[crate::store::OffHarness], machine_id: &str, agent: &str) -> bool {
    off.iter()
        .any(|harness| harness.machine_id == machine_id && harness.agent == agent)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn settings(env: Vec<HarnessEnv>) -> HarnessSettings {
        HarnessSettings {
            enabled: true,
            program: "  /opt/bin/codex-acp ".into(),
            home: "~/.codex".into(),
            args: vec!["--profile".into(), " ".into(), "work".into()],
            env,
        }
    }

    fn var(name: &str, value: Option<&str>) -> HarnessEnv {
        HarnessEnv {
            name: name.into(),
            value: value.map(SecretText::new),
        }
    }

    #[test]
    fn settings_are_trimmed_and_checked() {
        let ok = checked(settings(vec![
            var(" HTTPS_PROXY ", Some("x")),
            var("", None),
        ]))
        .unwrap();
        assert_eq!(ok.program, "/opt/bin/codex-acp");
        assert_eq!(ok.args, ["--profile", "work"]);
        assert_eq!(ok.env.len(), 1);
        assert_eq!(ok.env[0].name, "HTTPS_PROXY");
        assert!(checked(settings(vec![var("1BAD", Some("x"))])).is_err());
        assert!(checked(settings(vec![var("A", Some("x")), var("A", None)])).is_err());
        let mut broken = settings(Vec::new());
        broken.home = "a\nb".into();
        assert!(checked(broken).is_err());
        assert!(checked_agent("claude").is_ok());
        assert!(checked_agent("my-agent").is_ok());
        assert!(checked_agent("../x").is_err());
    }

    #[test]
    fn a_var_without_a_value_keeps_the_saved_one() {
        let kept = vec![
            EnvSecret {
                name: "HTTPS_PROXY".into(),
                value: SecretText::new("old"),
            },
            EnvSecret {
                name: "GONE".into(),
                value: SecretText::new("dropped"),
            },
        ];
        let merged = merged_env(
            kept,
            vec![
                var("HTTPS_PROXY", None),
                var("NEW", Some("fresh")),
                var("NEVER_SET", None),
            ],
        );
        let pairs: Vec<(&str, &str)> = merged
            .iter()
            .map(|var| (var.name.as_str(), var.value.expose()))
            .collect();
        assert_eq!(pairs, [("HTTPS_PROXY", "old"), ("NEW", "fresh")]);
    }

    #[tokio::test]
    async fn saved_vars_that_do_not_open_stop_the_save() {
        let db = crate::store::TestDb::new().await.unwrap();
        let user = User::for_test(db.store().await.unwrap(), "alice");
        let key = CredentialKey::random();
        let saved = vec![EnvSecret {
            name: "HTTPS_PROXY".into(),
            value: SecretText::new("old"),
        }];
        user.save_harness_settings(
            &key,
            SaveHarness {
                machine_id: "m1",
                agent: "codex",
                enabled: true,
                program: "",
                home: "",
                args: &[],
                env: &saved,
            },
        )
        .await
        .unwrap();
        let wanted = || vec![var("HTTPS_PROXY", None)];
        let kept = env_to_save(&user, &key, "m1", "codex", wanted())
            .await
            .unwrap();
        assert_eq!(kept, saved);
        let other = CredentialKey::random();
        assert!(
            env_to_save(&user, &other, "m1", "codex", wanted())
                .await
                .is_err()
        );
    }

    #[test]
    fn the_view_shows_names_never_values_and_beeps_token_first() {
        let info = HarnessInfo {
            agent: "claude".into(),
            installed: true,
            runnable: true,
            version: Some("2.1.312".into()),
            signed_in: Some(false),
            ..HarnessInfo::default()
        };
        let rows = vec![HarnessRow {
            machine_id: "m1".into(),
            agent: "claude".into(),
            enabled: false,
            program: String::new(),
            home: "~/.claude-work".into(),
            args: Vec::new(),
            env_names: vec!["HTTPS_PROXY".into()],
        }];
        let shown = harness_view(info.clone(), &rows, &[]);
        assert_eq!(shown.sign_in, HarnessSignIn::SignedOut {});
        assert!(!shown.settings.enabled);
        assert_eq!(shown.settings.env, [var("HTTPS_PROXY", None)]);
        assert_eq!(shown.home_var.as_deref(), Some("CLAUDE_CONFIG_DIR"));
        let token = AgentTokenRow {
            agent: AgentAccount::Claude,
            last4: "abcd".into(),
            created_at: Utc::now(),
            unreadable: false,
        };
        let shown = harness_view(info, &[], &[token]);
        assert_eq!(
            shown.sign_in,
            HarnessSignIn::BeepToken {
                last4: "abcd".into()
            }
        );
        assert!(shown.settings.enabled);
    }

    fn row(machine: &str, enabled: bool, program: &str) -> HarnessRow {
        HarnessRow {
            machine_id: machine.into(),
            agent: "codex".into(),
            enabled,
            program: program.into(),
            home: String::new(),
            args: Vec::new(),
            env_names: Vec::new(),
        }
    }

    #[test]
    fn saving_everywhere_copies_the_binary_path_only_when_asked() {
        let machines = ["m1".to_owned(), "m2".to_owned(), "m3".to_owned()];
        let rows = vec![row("m1", true, "/old"), row("m2", false, "/srv/codex")];
        let edited = checked(settings(Vec::new())).unwrap();
        let everywhere = |include_program| Everywhere {
            machines: &machines,
            from_machine: "m1",
            agent: "codex",
            include_program,
        };
        let kept = |machine: &str, enabled: bool, program: &str| Kept {
            machine_id: machine.into(),
            enabled,
            program: program.into(),
        };
        // Off by default: each other machine keeps its own path, and stays
        // off where it was off; one with nothing saved stays on.
        assert_eq!(
            kept_per_machine(&everywhere(false), &rows, &edited),
            [
                kept("m1", true, "/opt/bin/codex-acp"),
                kept("m2", false, "/srv/codex"),
                kept("m3", true, ""),
            ]
        );
        assert_eq!(
            kept_per_machine(&everywhere(true), &rows, &edited),
            [
                kept("m1", true, "/opt/bin/codex-acp"),
                kept("m2", false, "/opt/bin/codex-acp"),
                kept("m3", true, "/opt/bin/codex-acp"),
            ]
        );
    }

    #[tokio::test]
    async fn saving_everywhere_reaches_a_machine_with_nothing_saved() {
        let db = crate::store::TestDb::new().await.unwrap();
        let user = User::for_test(db.store().await.unwrap(), "alice");
        let key = CredentialKey::random();
        let secret = vec![EnvSecret {
            name: "HTTPS_PROXY".into(),
            value: SecretText::new("http://proxy:3128"),
        }];
        user.save_harness_settings(
            &key,
            SaveHarness {
                machine_id: "m1",
                agent: "codex",
                enabled: true,
                program: "",
                home: "",
                args: &[],
                env: &secret,
            },
        )
        .await
        .unwrap();
        // m2 is offline and has nothing saved: it gets the settings from
        // the store when it next connects.
        let machines = ["m1".to_owned(), "m2".to_owned()];
        let everywhere = Everywhere {
            machines: &machines,
            from_machine: "m1",
            agent: "codex",
            include_program: false,
        };
        let saved = save_everywhere(
            &user,
            &key,
            &everywhere,
            checked(settings(vec![var("HTTPS_PROXY", None)])).unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(saved, ["m1", "m2"]);
        let later = user.agent_settings(&key, "m2").await.unwrap();
        let codex = later.harnesses.iter().find(|h| h.agent == "codex").unwrap();
        assert_eq!(codex.args, ["--profile", "work"]);
        assert_eq!(codex.program, None);
        // The kept value was opened on m1 and sealed again for m2.
        assert_eq!(codex.env, secret);
        assert_eq!(user.harness_env(&key, "m2", "codex").await.unwrap(), secret);
    }

    #[test]
    fn off_harnesses_are_found_by_machine_and_agent() {
        let off = vec![crate::store::OffHarness {
            machine_id: "m1".into(),
            agent: "codex".into(),
        }];
        assert!(is_off(&off, "m1", "codex"));
        assert!(!is_off(&off, "m2", "codex"));
        assert!(!is_off(&off, "m1", "claude"));
    }
}
