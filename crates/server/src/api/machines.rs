//! The Machines screen of released apps. The server reaches no machine: lists are empty and
//! actions are refused; a desktop lists and runs them over its mesh.

use super::{Reply, devices::unlink, own_nodes::retired};
use crate::{api::router::Cx, protocol::*};
use iroh::EndpointId;

pub(super) async fn list_machines(_: Cx, _: ListMachines) -> Reply<Vec<Machine>> {
    Ok(Vec::new())
}

/// Unlinks one of the machines the account recorded.
pub(super) async fn remove_machine(
    cx: Cx,
    RemoveMachine { machine_id }: RemoveMachine,
) -> Reply<()> {
    let key: EndpointId = machine_id.parse().map_err(|_| ApiError::NotFound {})?;
    if key == cx.key {
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

/// Adding a machine over ssh was removed; released apps still ask.
pub(super) async fn list_ssh_hosts(_: Cx, _: ListSshHosts) -> Reply<Vec<String>> {
    Ok(Vec::new())
}

/// Adding a machine over ssh was removed; released apps still ask.
pub(super) async fn add_machine_over_ssh(_: Cx, _: AddMachineOverSsh) -> Reply<SshAdd> {
    Ok(SshAdd {
        machine_id: None,
        system: String::new(),
        error: "Adding a machine over SSH was removed. Add machines from a desktop.".into(),
        output: Vec::new(),
    })
}

pub(super) async fn update_machine(_: Cx, _: UpdateMachine) -> Reply<MachineUpdate> {
    retired()
}

pub(super) async fn get_machine_log(_: Cx, _: GetMachineLog) -> Reply<String> {
    retired()
}

pub(super) async fn list_updatable_machines(
    _: Cx,
    _: ListUpdatableMachines,
) -> Reply<Vec<UpdatableMachine>> {
    Ok(Vec::new())
}

pub(super) async fn list_machine_details(
    _: Cx,
    _: ListMachineDetails,
) -> Reply<Vec<MachineDetails>> {
    Ok(Vec::new())
}

pub(super) async fn list_machine_stats(_: Cx, _: ListMachineStats) -> Reply<Vec<MachineStats>> {
    Ok(Vec::new())
}

/// Setup tokens linked machines to the server, which takes none now.
pub(super) async fn create_node_token(_: Cx, _: CreateNodeToken) -> Reply<NodeToken> {
    retired()
}

/// No token is claimed any more.
pub(super) async fn get_node_token(_: Cx, _: GetNodeToken) -> Reply<Option<String>> {
    Err(ApiError::NotFound {})
}

/// No token can be claimed, so there is nothing to revoke.
pub(super) async fn revoke_node_token(_: Cx, _: RevokeNodeToken) -> Reply<()> {
    Ok(())
}
