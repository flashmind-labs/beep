//! Membership, invitations and device inventory.

use super::{DeviceStatus, Mesh};
use crate::store::{TicketPurpose, View};
use crate::{Device, DeviceKind, Invitation, JoinTicket, Key, MeshError};
use iroh::EndpointId;
use std::collections::BTreeSet;
use std::time::Duration;

/// Existing readers treat any observer value as a non-host controller.
const DESKTOP_OBSERVER: &[u8] = b"desktop";

impl Mesh {
    /// The authenticated device that redeemed this desktop's invite.
    /// A different ticket's join never counts, even when it happened at the same time.
    pub async fn joined_with(&self, invite: &Invitation) -> Result<Option<EndpointId>, MeshError> {
        if invite.addr().id != self.me() {
            return Err(MeshError::OtherMesh);
        }

        let secret = invite.secret().clone();
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

    /// Whether this observer is a desktop controller rather than a phone.
    pub fn is_desktop_observer(&self, id: EndpointId) -> bool {
        self.get(&Key::Observer(id.to_string())).as_deref() == Some(DESKTOP_OBSERVER)
    }

    /// Publishes this desktop's hosting mode without changing signed membership.
    /// Phone observers keep their existing empty marker.
    pub async fn set_desktop_hosting(&self, hosting: bool) -> Result<(), MeshError> {
        let key = Key::Observer(self.me().to_string());
        let value = (!hosting).then(|| DESKTOP_OBSERVER.to_vec());

        if self.get(&key) != value {
            self.set(key, value).await?;
        }

        Ok(())
    }

    /// Every machine in the mesh, including phones and offline members.
    pub fn machines(&self) -> Vec<DeviceStatus> {
        self.devices()
    }

    /// Machines that can host sessions and sync files, including offline hosts.
    pub fn code_hosts(&self) -> Vec<DeviceStatus> {
        self.machines()
            .into_iter()
            .filter(|status| !self.is_observer(status.device.id))
            .collect()
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

    /// Shows `id` as `name` on every member, or by the name it joined with when `name` is
    /// `None`. Desktops only.
    pub async fn rename(&self, id: EndpointId, name: Option<String>) -> Result<(), MeshError> {
        self.set(
            Key::MachineName(id.to_string()),
            name.map(String::into_bytes),
        )
        .await
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
        self.inner.me()
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

    /// What the mesh calls `id`, or its short id.
    pub fn name_of(&self, id: EndpointId) -> String {
        let view = self.inner.view();

        view.devices
            .members()
            .into_iter()
            .find(|device| device.id == id)
            .map_or_else(
                || id.fmt_short().to_string(),
                |device| shown_name(&view, &device),
            )
    }

    /// The members, each under the name the user gave it, if any.
    pub fn devices(&self) -> Vec<DeviceStatus> {
        let view = self.inner.view();
        let conns = self.inner.conns();
        let addrs = self.inner.addrs();
        let last_seen = self.inner.last_seen();
        view.devices
            .members()
            .into_iter()
            .map(|mut device| {
                device.name = shown_name(&view, &device);

                let me = device.id == self.inner.me();
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

/// The name the user gave `device`, or the one it joined with.
fn shown_name(view: &View, device: &Device) -> String {
    view.values
        .get(&Key::MachineName(device.id.to_string()))
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
        .filter(|name| !name.trim().is_empty())
        .map_or_else(|| device.name.clone(), str::to_owned)
}
