//! Membership, invitations and device inventory.

use super::{DeviceStatus, Mesh};
use crate::machine::Device as InventoryDevice;
use crate::store::TicketPurpose;
use crate::{DeviceKind, JoinTicket, Key, MeshError};
use iroh::EndpointId;
use std::collections::BTreeSet;
use std::time::Duration;

impl Mesh {
    /// The authenticated device that redeemed this ticket on this desktop.
    /// A different ticket's join never counts, even when it happened at the same time.
    pub async fn joined_with(&self, ticket: &JoinTicket) -> Result<Option<EndpointId>, MeshError> {
        if ticket.mesh != self.id() {
            return Err(MeshError::OtherMesh);
        }

        let secret = ticket.secret.clone();
        self.inner
            .store
            .run(move |core| Ok(core.joined_with(&secret)))
            .await
    }

    /// Mints a single-use ticket. A node cannot.
    pub async fn invite(&self, kind: DeviceKind, ttl: Duration) -> Result<JoinTicket, MeshError> {
        let addr = self.inner.endpoint.addr();
        self.inner
            .store
            .run(move |core| core.invite(addr, kind, ttl, TicketPurpose::Member))
            .await
    }

    /// Mints a single-use ticket for a temporary node, such as a container: desktops remove it
    /// once it has been offline a day. A node cannot.
    pub async fn invite_temporary(&self, ttl: Duration) -> Result<JoinTicket, MeshError> {
        let addr = self.inner.endpoint.addr();
        self.inner
            .store
            .run(move |core| core.invite(addr, DeviceKind::Node, ttl, TicketPurpose::Temporary))
            .await
    }

    /// Mints a phone ticket, retaining controller permissions without offering a code host.
    pub async fn invite_observer(&self, ttl: Duration) -> Result<JoinTicket, MeshError> {
        let addr = self.inner.endpoint.addr();
        self.inner
            .store
            .run(move |core| core.invite(addr, DeviceKind::Desktop, ttl, TicketPurpose::Observer))
            .await
    }

    /// Whether `id` is an observer rather than a code host.
    pub fn is_observer(&self, id: EndpointId) -> bool {
        self.inner
            .view()
            .values
            .contains_key(&Key::Observer(id.to_string()))
    }

    /// Every machine in the mesh, including phones and offline members.
    pub fn machines(&self) -> Vec<DeviceStatus> {
        self.devices()
    }

    /// Machines that can host sessions and sync files, including offline hosts.
    pub fn code_hosts(&self) -> Vec<DeviceStatus> {
        self.inventory()
            .into_iter()
            .filter_map(|device| match device {
                InventoryDevice::Node(status) | InventoryDevice::Desktop(status) => Some(status),
                InventoryDevice::Phone(_) => None,
            })
            .collect()
    }

    /// Whether `id` joined with a temporary ticket and was not removed since.
    pub fn is_temporary(&self, id: EndpointId) -> bool {
        self.inner
            .view()
            .values
            .contains_key(&Key::Temporary(id.to_string()))
    }

    /// The temporary devices.
    pub fn temporaries(&self) -> BTreeSet<EndpointId> {
        self.inner
            .view()
            .values
            .keys()
            .filter_map(|key| match key {
                Key::Temporary(id) => id.parse().ok(),
                _ => None,
            })
            .collect()
    }

    /// Removes the temporary device `id` for good. `Ok(false)` when it was removed already;
    /// a device that is not temporary is refused. A node cannot.
    pub async fn remove_temporary(&self, id: EndpointId) -> Result<bool, MeshError> {
        let removed = self
            .inner
            .store
            .run(move |core| core.remove_temporary(id))
            .await?;
        // No receiver is a normal state.
        self.inner.changes.send(Key::Temporary(id.to_string())).ok();
        Ok(removed)
    }

    /// Removes a device for good. A node cannot.
    pub async fn remove(&self, id: EndpointId) -> Result<(), MeshError> {
        self.inner.store.run(move |core| core.remove(id)).await
    }

    /// Whether `id` was removed from the mesh, for good.
    pub fn is_removed(&self, id: EndpointId) -> bool {
        self.inner.view().devices.is_removed(id)
    }

    /// This device's id.
    pub fn me(&self) -> EndpointId {
        self.inner.me
    }

    /// The mesh's id: the id of the desktop that started it.
    pub fn id(&self) -> EndpointId {
        self.inner.view().devices.mesh()
    }

    /// Whether `id` is a device of the mesh now. Asked per connection and
    /// per request, so a removed device is out at once.
    pub fn is_member(&self, id: EndpointId) -> bool {
        self.inner.is_member(id)
    }

    pub fn devices(&self) -> Vec<DeviceStatus> {
        let view = self.inner.view();
        let conns = self.inner.conns();
        let addrs = self.inner.addrs();
        let last_seen = self.inner.last_seen();
        view.devices
            .members()
            .into_iter()
            .map(|device| {
                let me = device.id == self.inner.me;
                let online = me
                    || conns
                        .get(&device.id)
                        .is_some_and(|l| l.conn.close_reason().is_none());
                DeviceStatus {
                    online,
                    last_seen: (!online)
                        .then(|| last_seen.get(&device.id).copied())
                        .flatten(),
                    addr: if me {
                        Some(self.inner.endpoint.addr())
                    } else {
                        addrs.get(&device.id).map(|n| n.addr.clone())
                    },
                    device,
                }
            })
            .collect()
    }
}
