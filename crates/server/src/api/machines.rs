//! The machines running beep-node, for the app's Machines screen.

use super::Reply;
use crate::{
    instance::Privilege,
    protocol::{node::EnvReport, *},
    router::Cx,
    store::NodeRow,
};
use std::collections::HashSet;

/// Admins only, like nodes themselves. A machine whose key was unlinked
/// is left out.
pub(super) async fn list_machines(cx: Cx, _: ListMachines) -> Reply<Vec<Machine>> {
    if !cx.app.allows(Privilege::Nodes, &cx.user) {
        return Err(ApiError::Unauthorized {});
    }
    let keys: HashSet<String> = cx
        .user
        .account_keys()
        .await?
        .into_iter()
        .map(|key| key.endpoint_id)
        .collect();
    Ok(cx
        .user
        .nodes()
        .await?
        .into_iter()
        .filter(|node| keys.contains(&node.endpoint))
        .map(|node| {
            let online = cx.app.nodes.online(&node.endpoint).is_some();
            machine(node, online)
        })
        .collect())
}

fn machine(node: NodeRow, online: bool) -> Machine {
    Machine {
        id: node.endpoint,
        name: node.name,
        online,
        seen_at: node.seen_at.into(),
        version: node.version,
        env: node.env.map(machine_env),
    }
}

fn machine_env(env: EnvReport) -> MachineEnv {
    MachineEnv {
        os: env.os,
        os_version: env.os_version,
        arch: env.arch,
        hostname: env.hostname,
        cpus: env.cpus,
        memory_mb: env.memory_mb,
        home: env.home,
        tools: env
            .tools
            .into_iter()
            .map(|tool| MachineTool {
                name: tool.name,
                version: tool.version,
            })
            .collect(),
        logins: env
            .logins
            .into_iter()
            .map(|login| MachineLogin {
                tool: login.tool,
                signed_in: login.signed_in,
                account: login.account,
            })
            .collect(),
        avds: env.avds,
    }
}
