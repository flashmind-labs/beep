//! The machines running beep-node, for the app's Machines screen.

use super::Reply;
use crate::{
    instance::Privilege,
    nodes::{self, TokenRefusal},
    protocol::{
        node::{AppInfo, AppName, Capability, EnvReport, GpuInfo},
        setup::SetupToken,
        *,
    },
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

/// Admins only, like `list_machines`. A machine that has not reported
/// yet is listed with nothing in it.
pub(super) async fn list_machine_details(
    cx: Cx,
    _: ListMachineDetails,
) -> Reply<Vec<MachineDetails>> {
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
        .map(|node| machine_details(node.endpoint, node.env.unwrap_or_default()))
        .collect())
}

/// A setup token for a new machine. Admins only, like `list_machines`.
pub(super) async fn create_node_token(
    cx: Cx,
    CreateNodeToken { name }: CreateNodeToken,
) -> Reply<NodeToken> {
    match nodes::create_token(&cx.app, &cx.user, name.as_deref()).await {
        Ok(made) => Ok(NodeToken {
            token: made.token.to_string(),
            expires_at: made.expires_at.into(),
        }),
        Err(TokenRefusal::NotAllowed) => Err(ApiError::Unauthorized {}),
        Err(TokenRefusal::Failed(e)) => Err(e.into()),
        Err(refusal) => Err(ApiError::bad_request(refusal.to_string())),
    }
}

/// Only the caller's own unused tokens.
pub(super) async fn revoke_node_token(
    cx: Cx,
    RevokeNodeToken { token }: RevokeNodeToken,
) -> Reply<()> {
    let token = SetupToken::parse(&token).map_err(|e| ApiError::bad_request(e.to_string()))?;
    if cx.user.revoke_node_token(&token.hash()).await? {
        Ok(())
    } else {
        Err(ApiError::NotFound {})
    }
}

fn machine_details(id: String, env: EnvReport) -> MachineDetails {
    MachineDetails {
        id,
        capabilities: env
            .capabilities()
            .into_iter()
            .map(machine_capability)
            .collect(),
        apps: env
            .apps
            .into_iter()
            .filter(|app| app.name != AppName::Unknown)
            .map(machine_app)
            .collect(),
        gpus: env.gpus.into_iter().map(machine_gpu).collect(),
    }
}

/// "Unreal Engine 5.4" for the engine, whose versions are part of its name;
/// the others keep theirs in `version`.
fn machine_app(app: AppInfo) -> MachineApp {
    match (app.name, &app.version) {
        (AppName::UnrealEngine, Some(version)) => MachineApp {
            name: format!("{} {version}", app.name),
            version: app.version,
        },
        _ => MachineApp {
            name: app.name.to_string(),
            version: app.version,
        },
    }
}

fn machine_gpu(gpu: GpuInfo) -> MachineGpu {
    MachineGpu {
        vendor: gpu.vendor.to_string(),
        model: gpu.model,
        vram_mb: gpu.vram_mb,
        driver: gpu.driver,
    }
}

pub(super) fn machine_capability(capability: Capability) -> MachineCapability {
    match capability {
        Capability::Gpu => MachineCapability::Gpu {},
        Capability::Cuda => MachineCapability::Cuda {},
        Capability::Blender => MachineCapability::Blender {},
        Capability::Unreal => MachineCapability::Unreal {},
        Capability::Xcode => MachineCapability::Xcode {},
        Capability::Android => MachineCapability::Android {},
        Capability::Docker => MachineCapability::Docker {},
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn details_leave_out_unknown_apps() {
        let env = EnvReport {
            apps: vec![
                AppInfo {
                    name: AppName::Unknown,
                    version: None,
                },
                AppInfo {
                    name: AppName::Blender,
                    version: Some("4.2".into()),
                },
            ],
            ..EnvReport::default()
        };
        let details = machine_details("id".into(), env);
        assert_eq!(details.apps.len(), 1);
        assert_eq!(details.apps[0].name, "Blender");
    }
}
