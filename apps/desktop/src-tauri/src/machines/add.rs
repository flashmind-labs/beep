//! The "Add a machine" sheet: a one-time ticket from this Mac that the
//! machine redeems with `beep-node mesh join <ticket>`. While open, the
//! sheet polls `node_token_machine` to learn which machine joined with it.
//! Only a desktop mints tickets: the server never does.

use crate::{
    beep::{Service, identity::computer_name},
    error::{DesktopError, Result},
    fleet::{Fleet, Machines, files as direct},
};
use beep_mesh::{DEFAULT_TICKET_TTL, DeviceKind, DeviceStatus, JoinTicket};
use beep_proto::{GetMachineSearch, SetMachineSearch};
use iroh::EndpointId;
use iroh_tickets::Ticket;
use serde::Serialize;
use std::{sync::Arc, time::Duration};
use tauri::State;

/// A ticket as the sheet shows it.
#[derive(Debug, Serialize)]
pub struct TokenView {
    /// The ticket, as `beep-node mesh join` takes it.
    token: String,
    /// `beep-node mesh join <ticket>`, to copy.
    command: String,
    /// In milliseconds since the Unix epoch.
    expires_at: u64,
}

/// A ticket that brings a device of `kind` into the mesh, as text.
#[tauri::command]
pub async fn mesh_invite(fleet: State<'_, Arc<Fleet>>, kind: DeviceKind) -> Result<String> {
    Ok(fleet.invite(kind).await?.encode_string())
}

/// Joins the mesh a ticket from another desktop names, under this Mac's name. A Mac that is
/// alone in its own mesh leaves it and brings its settings along.
#[tauri::command]
pub async fn mesh_join(fleet: State<'_, Arc<Fleet>>, ticket: String) -> Result<()> {
    let ticket = JoinTicket::decode_string(ticket.trim())
        .map_err(|_| DesktopError::BadRequest("that is not a ticket".into()))?;
    fleet.join(&ticket, &computer_name()).await
}

#[tauri::command]
pub async fn create_node_token(fleet: State<'_, Arc<Fleet>>) -> Result<TokenView> {
    let ticket = fleet.invite(DeviceKind::Node).await?;
    let token = ticket.encode_string();
    Ok(TokenView {
        command: format!("beep-node mesh join {token}"),
        token,
        expires_at: ticket.expires_ms,
    })
}

/// The id of the machine that joined with `token`, `None` while it is
/// unused. A machine is that one when it is a member of the ticket's kind
/// that was added after the ticket was minted.
#[tauri::command]
pub async fn node_token_machine(
    fleet: State<'_, Arc<Fleet>>,
    token: String,
) -> Result<Option<String>> {
    let ticket = JoinTicket::decode_string(token.trim())
        .map_err(|_| DesktopError::BadRequest("that is not a ticket".into()))?;
    let joined = claimed(&ticket, DEFAULT_TICKET_TTL, fleet.machines(), fleet.id());
    Ok(joined.map(|id| id.to_string()))
}

/// The member that joined with `ticket`, which was minted `ttl` before it
/// expired. Tickets are single use, so a new member of its kind is the one.
fn claimed(
    ticket: &JoinTicket,
    ttl: Duration,
    members: Vec<DeviceStatus>,
    me: EndpointId,
) -> Option<EndpointId> {
    let minted = ticket
        .expires_ms
        .saturating_sub(u64::try_from(ttl.as_millis()).unwrap_or(u64::MAX));
    members
        .into_iter()
        .map(|status| status.device)
        .find(|device| {
            device.id != me && device.kind == ticket.kind && device.added_at_ms >= minted
        })
        .map(|device| device.id)
}

/// Where `machine_id` looks for git checkouts, `~/` for home.
#[tauri::command]
pub async fn machine_search(
    beep: Service<'_>,
    fleet: Machines<'_>,
    machine_id: String,
) -> Result<String> {
    if let Some(machine) = fleet.member(&machine_id) {
        return direct::machine_search(&fleet, machine).await;
    }
    beep.call(GetMachineSearch { machine_id }).await
}

/// Has `machine_id` look for git checkouts in `search` and walk it again.
#[tauri::command]
pub async fn set_machine_search(
    beep: Service<'_>,
    fleet: Machines<'_>,
    machine_id: String,
    search: String,
) -> Result<String> {
    if let Some(machine) = fleet.member(&machine_id) {
        return direct::set_machine_search(&fleet, machine, &search).await;
    }
    beep.call(SetMachineSearch { machine_id, search }).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use beep_mesh::{Device, JoinSecret};
    use iroh::SecretKey;

    fn member(kind: DeviceKind, added_at_ms: u64) -> DeviceStatus {
        DeviceStatus {
            device: Device {
                id: SecretKey::generate().public(),
                name: "box".to_owned(),
                kind,
                added_at_ms,
            },
            online: true,
            addr: None,
        }
    }

    #[test]
    fn a_ticket_names_the_machine_that_joined_after_it_was_minted() {
        let me = SecretKey::generate().public();
        let ttl = Duration::from_secs(60);
        let ticket = JoinTicket {
            mesh: me,
            addr: iroh::EndpointAddr::new(me),
            kind: DeviceKind::Node,
            secret: JoinSecret::generate(),
            expires_ms: 100_000,
        };
        let before = member(DeviceKind::Node, 39_999);
        let desktop = member(DeviceKind::Desktop, 50_000);
        let joined = member(DeviceKind::Node, 60_000);
        let members = vec![before, desktop, joined.clone()];
        assert_eq!(
            claimed(&ticket, ttl, members.clone(), me),
            Some(joined.device.id)
        );
        assert_eq!(claimed(&ticket, ttl, members[..2].to_vec(), me), None);
    }
}
