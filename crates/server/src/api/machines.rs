//! The machines running beep-node, for the app's Machines screen.

use super::{Reply, devices::unlink, ssh::owned_online};
use crate::{
    api::router::Cx,
    nodes::{self, Sampled, TokenRefusal},
    protocol::{
        node::{AppInfo, AppName, Capability, DiskUsage, EnvReport, GpuInfo},
        setup::SetupToken,
        *,
    },
    store::NodeRow,
};
use futures::{StreamExt, stream::FuturesOrdered};
use iroh::EndpointId;
use std::collections::HashSet;

/// The caller's own machines, without unlinked keys.
pub(super) async fn list_machines(cx: Cx, _: ListMachines) -> Reply<Vec<Machine>> {
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

/// Unlinks one of the caller's machines without waiting on it, dropping any connection.
pub(super) async fn remove_machine(
    cx: Cx,
    RemoveMachine { machine_id }: RemoveMachine,
) -> Reply<()> {
    let key: EndpointId = machine_id.parse().map_err(|_| ApiError::NotFound {})?;
    if key == cx.connection.remote_id() {
        return Err(ApiError::bad_request(
            "This is the machine you are on. Unlink it from Settings.",
        ));
    }
    let listed = cx
        .user
        .nodes()
        .await?
        .iter()
        .any(|node| node.endpoint == machine_id);
    if !listed {
        return Err(ApiError::NotFound {});
    }
    unlink(&cx, key).await
}

/// Has one of the caller's online machines update its beep-node. Another
/// account's machine is `NotFound`, as one that does not exist.
pub(super) async fn update_machine(
    cx: Cx,
    UpdateMachine {
        machine_id,
        stop_sessions,
    }: UpdateMachine,
) -> Reply<MachineUpdate> {
    let owned = cx
        .user
        .nodes()
        .await?
        .into_iter()
        .find(|node| node.endpoint == machine_id)
        .ok_or(ApiError::NotFound {})?;
    let online = cx
        .app
        .nodes
        .online(&machine_id)
        .filter(|online| online.user == cx.user.id())
        .ok_or_else(|| ApiError::bad_request(format!("{} is offline.", owned.name)))?;
    if !online.updatable() {
        return Err(ApiError::bad_request(format!(
            "This beep-node cannot be updated from Beep. Run `beep-node update` on {}.",
            owned.name
        )));
    }
    let updated = online.update(stop_sessions).await.map_err(|e| {
        log::info!("node update failed; node={machine_id} error={e:#}");
        ApiError::bad_request(format!("{e:#}"))
    })?;
    log::info!(
        "node updated; node={machine_id} from={} to={} restarting={}",
        updated.from,
        updated.to,
        updated.restarting
    );
    Ok(MachineUpdate {
        from: updated.from,
        to: updated.to,
        restarting: updated.restarting,
        note: updated.note,
    })
}

/// The last lines of one of the caller's online machines' log. Another
/// account's machine is `NotFound`, as one that does not exist.
pub(super) async fn get_machine_log(
    cx: Cx,
    GetMachineLog { machine_id, lines }: GetMachineLog,
) -> Reply<String> {
    let (_, online) = owned_online(&cx, &machine_id).await?;
    online
        .log(lines)
        .await
        .map_err(|e| ApiError::bad_request(format!("{e:#}")))
}

/// The caller's online machines that `update_machine` can update.
pub(super) async fn list_updatable_machines(
    cx: Cx,
    _: ListUpdatableMachines,
) -> Reply<Vec<UpdatableMachine>> {
    Ok(cx
        .app
        .nodes
        .of_user(cx.user.id())
        .into_iter()
        .filter(|online| online.updatable())
        .map(|online| async move {
            UpdatableMachine {
                id: online.connection.remote_id().to_string(),
                // Uncounted, the update itself asks.
                agent_sessions: online.running_agent_sessions().await.unwrap_or(0),
            }
        })
        .collect::<FuturesOrdered<_>>()
        .collect()
        .await)
}

/// Like `list_machines`, with details; one not reported yet is listed empty.
pub(super) async fn list_machine_details(
    cx: Cx,
    _: ListMachineDetails,
) -> Reply<Vec<MachineDetails>> {
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

/// The caller's online machines that sent stats on this connection.
pub(super) async fn list_machine_stats(cx: Cx, _: ListMachineStats) -> Reply<Vec<MachineStats>> {
    Ok(cx
        .app
        .nodes
        .of_user(cx.user.id())
        .into_iter()
        .filter_map(|online| {
            let id = online.connection.remote_id().to_string();
            Some(machine_stats(id, online.stats()?))
        })
        .collect())
}

/// A setup token that links a new machine to the caller.
pub(super) async fn create_node_token(
    cx: Cx,
    CreateNodeToken { name }: CreateNodeToken,
) -> Reply<NodeToken> {
    match nodes::create_token(&cx.app, &cx.user, name.as_deref()).await {
        Ok(made) => Ok(NodeToken {
            token: made.token.to_string(),
            expires_at: made.expires_at.into(),
        }),
        Err(TokenRefusal::Failed(e)) => Err(e.into()),
        Err(refusal) => Err(ApiError::bad_request(refusal.to_string())),
    }
}

/// Only the caller's own tokens.
pub(super) async fn get_node_token(
    cx: Cx,
    GetNodeToken { token }: GetNodeToken,
) -> Reply<Option<String>> {
    let token = SetupToken::parse(&token).map_err(|e| ApiError::bad_request(e.to_string()))?;
    cx.user
        .node_token_claimer(&token.hash())
        .await?
        .ok_or(ApiError::NotFound {})
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

/// "Unreal Engine 5.4" for the engine, whose versions are part of its name.
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

fn machine_stats(id: String, Sampled { at, report }: Sampled) -> MachineStats {
    MachineStats {
        id,
        sampled_at: at,
        disks: report.disks.into_iter().map(machine_disk).collect(),
        memory_used_mb: report.memory_used_mb,
        memory_total_mb: report.memory_total_mb,
        load: report.load,
    }
}

fn machine_disk(disk: DiskUsage) -> MachineDisk {
    MachineDisk {
        path: disk.path,
        free_bytes: disk.free_bytes,
        total_bytes: disk.total_bytes,
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
