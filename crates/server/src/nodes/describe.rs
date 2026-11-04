//! The user's machines as the agent reads them.

use super::Nodes;
use crate::{
    protocol::node::{AppName, EnvReport},
    store::NodeRow,
};

/// The machines section's first line, also how a chat finds the last one it got.
pub(crate) const MACHINES_HEADER: &str = "The user's machines (exec, process, desktop and \
    code_agent run on the one `in` names; send_file copies files between them). Pick the \
    machine whose OS and `can` list fit the task: iOS, iPadOS and macOS builds, Xcode and the \
    iOS simulator need a macOS machine that can xcode, whichever machine is the default:";

/// The machines section of a turn's context, one line per machine.
pub(crate) fn machines(nodes: &[NodeRow], online: &Nodes) -> Option<String> {
    if nodes.is_empty() {
        return None;
    }
    let mut out = String::from(MACHINES_HEADER);
    for node in nodes {
        let connected = online.online(&node.endpoint);
        let mut facts = vec![if connected.is_some() {
            "online".to_owned()
        } else {
            "offline".to_owned()
        }];
        if connected.is_some_and(|online| online.workspace.is_some()) {
            facts.push("the user's environment".to_owned());
        }
        if let Some(env) = &node.env {
            facts.push(env.os_version.clone());
            let capabilities: Vec<&'static str> =
                env.capabilities().into_iter().map(Into::into).collect();
            facts.push(if capabilities.is_empty() {
                "can: nothing special".to_owned()
            } else {
                format!("can: {}", capabilities.join(", "))
            });
            for login in &env.logins {
                facts.push(match (login.signed_in, &login.account) {
                    (true, Some(account)) => format!("{} signed in as {account}", login.tool),
                    (true, None) => format!("{} signed in", login.tool),
                    (false, _) => format!("{} not signed in", login.tool),
                });
            }
        }
        facts.retain(|fact| !fact.is_empty());
        out.push_str(&format!("\n- {}: {}", node.name, facts.join("; ")));
    }
    Some(out)
}

/// A node's report as a few lines for the model.
pub fn describe(env: &EnvReport) -> String {
    let mut lines = vec![format!(
        "{} {} ({}), {} CPUs, {} GB memory, host {}, home {}",
        env.os,
        env.os_version,
        env.arch,
        env.cpus,
        env.memory_mb / 1024,
        env.hostname,
        env.home
    )];
    if !env.tools.is_empty() {
        let tools: Vec<_> = env
            .tools
            .iter()
            .map(|tool| match &tool.version {
                Some(version) => format!("{} ({version})", tool.name),
                None => tool.name.clone(),
            })
            .collect();
        lines.push(format!("Tools: {}", tools.join(", ")));
    }
    for gpu in &env.gpus {
        lines.push(match gpu.vram_mb {
            Some(mb) => format!("GPU: {} ({} GB)", gpu.model, (mb + 512) / 1024),
            None => format!("GPU: {}", gpu.model),
        });
    }
    let known_apps: Vec<_> = env
        .apps
        .iter()
        .filter(|app| app.name != AppName::Unknown)
        .collect();
    if !known_apps.is_empty() {
        let apps: Vec<_> = known_apps
            .into_iter()
            .map(|app| match &app.version {
                Some(version) => format!("{} {version}", app.name),
                None => app.name.to_string(),
            })
            .collect();
        lines.push(format!("Apps: {}", apps.join(", ")));
    }
    let capabilities: Vec<&'static str> = env.capabilities().into_iter().map(Into::into).collect();
    if !capabilities.is_empty() {
        lines.push(format!("Can: {}", capabilities.join(", ")));
    }
    for login in &env.logins {
        lines.push(match (login.signed_in, &login.account) {
            (true, Some(account)) => format!("{}: signed in as {account}", login.tool),
            (true, None) => format!("{}: signed in", login.tool),
            (false, _) => format!("{}: not signed in", login.tool),
        });
    }
    if !env.avds.is_empty() {
        lines.push(format!("Android emulators: {}", env.avds.join(", ")));
    }
    lines.join("\n")
}
