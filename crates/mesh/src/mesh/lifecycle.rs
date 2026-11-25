//! Creating, opening, joining and closing a mesh.

use super::{AddrNote, Core, InUse, Mesh, blocking};
use crate::{DeviceKind, Devices, Invitation, MeshError};
use iroh::{Endpoint, SecretKey};
use std::path::Path;

impl Mesh {
    /// The mesh in `dir`, or `None` when this directory is not in one.
    pub async fn open(dir: &Path, endpoint: Endpoint) -> Result<Option<Mesh>, MeshError> {
        let dir = dir.to_owned();
        let key = endpoint.secret_key().clone();
        let loaded = {
            let dir = dir.clone();
            blocking(move || {
                Core::recover(&dir)?;
                let Some(devices) = Core::load(&dir)? else {
                    return Ok(None);
                };
                Ok(Some((
                    Core::start(&dir, key, devices)?,
                    Core::load_addrs(&dir),
                )))
            })
            .await?
        };

        match loaded {
            None => Ok(None),
            Some((core, addrs)) => Ok(Some(Self::run(endpoint, core, addrs)?)),
        }
    }

    /// Starts a mesh with this device, a desktop, as its only member.
    pub async fn create(dir: &Path, endpoint: Endpoint, name: &str) -> Result<Mesh, MeshError> {
        let (dir, name) = (dir.to_owned(), name.to_owned());
        let key = endpoint.secret_key().clone();
        let core = {
            let dir = dir.clone();
            blocking(move || {
                if Core::exists(&dir) {
                    return Err(MeshError::AlreadyMember);
                }

                let devices = crate::Devices::genesis(&key, &name, DeviceKind::Desktop);
                Core::start(&dir, key, devices)
            })
            .await?
        };
        Self::run(endpoint, core, Vec::new())
    }

    /// Joins the mesh `ticket` names. The kind comes from the desktop's
    /// signed `Add`, never from this side.
    pub async fn join(
        dir: &Path,
        endpoint: Endpoint,
        invite: impl Into<Invitation>,
        name: &str,
    ) -> Result<Mesh, MeshError> {
        let dir = dir.to_owned();

        if Core::exists(&dir) {
            return Err(MeshError::AlreadyMember);
        }

        let (devices, addrs, key) =
            redeem(&endpoint, &Into::<Invitation>::into(invite), name).await?;
        let core = {
            let (dir, addrs) = (dir.clone(), addrs.clone());
            blocking(move || {
                let core = Core::start(&dir, key, devices)?;
                core.save_addrs(&addrs)?;
                Ok(core)
            })
            .await?
        };
        Self::run(endpoint, core, addrs)
    }

    /// Deletes the mesh in `dir`, so the device is in none. `close` the `Mesh`
    /// first.
    pub async fn forget(dir: &Path) -> Result<(), MeshError> {
        let dir = dir.to_owned();
        blocking(move || Core::forget(&dir)).await
    }

    /// Returns once this is the only handle to the mesh, which is when
    /// `close` can take it: the others drop with the tasks that held them.
    pub async fn released(&self) {
        let mut handles = self._guard.0.handles.subscribe();
        let _ = beep_proto::watched::wait_for(&mut handles, |handles| *handles <= 1).await;
    }

    /// Stops this mesh and waits until its files are no longer written, so
    /// another can take the directory. It must be the only handle: with
    /// another clone alive the mesh is handed back in `InUse`, still running.
    pub async fn close(self) -> Result<(), InUse> {
        if beep_proto::watched::read_sent(&self._guard.0.handles, |handles| *handles) > 1 {
            return Err(InUse(self));
        }

        let inner = self.inner.clone();
        drop(self);
        inner.store.close().await;
        Ok(())
    }

    /// Joins the mesh `ticket` names in place of the one in `dir`, which
    /// must hold no device but this one, or have removed this one. `close` the old `Mesh` first. Nothing
    /// changes on disk until the ticket is redeemed (a ticket is spent by
    /// then: a crash after that needs a fresh one). The swap moves the old
    /// files aside and removes them once the new ones are on disk, and a
    /// crash during it leaves the old mesh or the new: `open` finishes or
    /// undoes it through `recover`. What the old mesh held is gone; read it
    /// first to carry it over.
    pub async fn join_replacing_lone(
        dir: &Path,
        endpoint: Endpoint,
        invite: impl Into<Invitation>,
        name: &str,
    ) -> Result<Mesh, MeshError> {
        let invite = Into::<Invitation>::into(invite);
        let dir = dir.to_owned();
        let me = endpoint.id();
        {
            let dir = dir.clone();
            blocking(move || {
                Core::recover(&dir)?;

                match Core::load(&dir)? {
                    // A device that was removed has nothing to keep.
                    Some(devices)
                        if devices.is_member(me)
                            && devices.members().iter().any(|d| d.id != me) =>
                    {
                        Err(MeshError::NotLone)
                    }
                    _ => Ok(()),
                }
            })
            .await?;
        }
        let (devices, addrs, key) = redeem(&endpoint, &invite, name).await?;
        let core = {
            let (dir, addrs) = (dir.clone(), addrs.clone());
            blocking(move || Core::swap_in(&dir, key, devices, &addrs)).await?
        };
        Self::run(endpoint, core, addrs)
    }
}

async fn redeem(
    endpoint: &Endpoint,
    invite: &Invitation,
    name: &str,
) -> Result<(Devices, Vec<AddrNote>, SecretKey), MeshError> {
    let welcome = crate::join::redeem(endpoint, invite, name).await?;
    let mut addrs = welcome.addrs;
    addrs.push(AddrNote {
        addr: invite.addr().clone(),
        generation: 0,
    });

    Ok((welcome.devices, addrs, endpoint.secret_key().clone()))
}
