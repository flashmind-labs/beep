//! Admitting a key: sign-in, linking, the instance owner and the bundled node.

use super::{Admission, App, SignedIn};
use crate::{
    account::{instance::Instance, login::LoginConfig},
    protocol::DeviceKind,
    store::{Account, LinkKey, NewKey, SignIn},
};
use anyhow::Result;
use iroh::EndpointId;
use std::sync::Arc;

impl App {
    /// The user `endpoint` is, started on first connection; `None` for a removed or refused key.
    pub async fn sign_in(self: &Arc<Self>, endpoint: &EndpointId) -> Result<Option<SignedIn>> {
        Ok(match self.admit(endpoint).await? {
            Admission::SignedIn(signed_in) => Some(signed_in),
            Admission::Revoked | Admission::NotInvited | Admission::Guest => None,
        })
    }

    /// Who `endpoint` signs in as; an unlinked key makes its own account unless single-user rules forbid.
    pub async fn admit(self: &Arc<Self>, endpoint: &EndpointId) -> Result<Admission> {
        let Some(instance) = &self.instance else {
            let sign_in = if self.login.is_some() {
                match self.store.linked_sign_in(endpoint).await? {
                    Some(sign_in) => sign_in,
                    None => return Ok(Admission::Guest),
                }
            } else {
                self.store.sign_in(endpoint).await?
            };
            return Ok(match sign_in {
                SignIn::Account {
                    account,
                    device,
                    primary,
                } => Admission::SignedIn(self.signed_in(&account, device, primary)),
                SignIn::Revoked => Admission::Revoked,
            });
        };
        let sign_in = match self.store.linked_sign_in(endpoint).await? {
            Some(sign_in) => sign_in,
            None => match self.claim_new(instance, endpoint).await? {
                Some(sign_in) => sign_in,
                None => return Ok(Admission::Guest),
            },
        };
        Ok(match sign_in {
            SignIn::Account {
                account,
                device,
                primary,
            } => {
                if !self.owns(instance, &account, endpoint).await? {
                    return Ok(Admission::NotInvited);
                }
                Admission::SignedIn(self.signed_in(&account, device, primary))
            }
            SignIn::Revoked => Admission::Revoked,
        })
    }

    /// Who an already-linked key signs in as; `None` for a revoked or unlinked key.
    pub async fn sign_in_linked(
        self: &Arc<Self>,
        endpoint: &EndpointId,
    ) -> Result<Option<SignedIn>> {
        let mut sign_in = self.store.linked_sign_in(endpoint).await?;
        if let Some(instance) = &self.instance
            && sign_in.is_none()
            && self.link_local_node(instance, endpoint).await?
        {
            sign_in = self.store.linked_sign_in(endpoint).await?;
        }
        Ok(match sign_in {
            Some(SignIn::Account {
                account,
                device,
                primary,
            }) => {
                if let Some(instance) = &self.instance
                    && !self.owns(instance, &account, endpoint).await?
                {
                    return Ok(None);
                }
                Some(self.signed_in(&account, device, primary))
            }
            Some(SignIn::Revoked) | None => None,
        })
    }

    pub(super) fn signed_in(
        self: &Arc<Self>,
        account: &Account,
        device: DeviceKind,
        primary: bool,
    ) -> SignedIn {
        SignedIn {
            user: self.start_user(account),
            device,
            primary,
        }
    }

    /// The website keys sign in through, on a hosted server.
    pub fn login(&self) -> Option<&LoginConfig> {
        self.login.as_ref()
    }

    /// The single-user settings, when the server runs for one owner.
    pub fn instance(&self) -> Option<&Instance> {
        self.instance.as_ref()
    }

    /// The instance owner's account, read once and then kept.
    pub async fn instance_owner(&self) -> Result<Option<String>> {
        let Some(instance) = &self.instance else {
            return Ok(None);
        };
        if let Some(owner) = instance.owner() {
            return Ok(Some(owner));
        }
        let owner = self.store.instance_owner().await?;
        if let Some(owner) = &owner {
            instance.set_owner(owner);
        }
        Ok(owner)
    }

    /// Makes `endpoint` the owner's new account when nobody owns the instance and the config allows.
    pub(super) async fn claim_new(
        &self,
        instance: &Instance,
        endpoint: &EndpointId,
    ) -> Result<Option<SignIn>> {
        if instance.owner_key().is_some_and(|key| key != endpoint)
            || self.instance_owner().await?.is_some()
        {
            log::info!("refused a key that is not the owner's; key={endpoint}");
            return Ok(None);
        }
        let claimed = self.store.claim_instance_new(endpoint).await?;
        if let Some(SignIn::Account { account, .. }) = &claimed {
            log::info!("instance claimed; user={} key={endpoint}", account.id);
            instance.set_owner(&account.id);
        }
        Ok(claimed)
    }

    /// Whether `account` owns the instance, claiming it when unowned and the config allows this key.
    pub(super) async fn owns(
        &self,
        instance: &Instance,
        account: &Account,
        endpoint: &EndpointId,
    ) -> Result<bool> {
        if let Some(owner) = self.instance_owner().await? {
            return Ok(owner == account.id);
        }
        if instance.owner_key().is_some_and(|key| key != endpoint) {
            return Ok(false);
        }
        if self.store.claim_instance(&account.id, endpoint).await? {
            log::info!("instance claimed; user={} key={endpoint}", account.id);
            instance.set_owner(&account.id);
            return Ok(true);
        }
        // Another key claimed it a moment ago.
        Ok(self.instance_owner().await?.as_deref() == Some(account.id.as_str()))
    }

    /// Links `endpoint` to the owner when it is the bundled node's key; `false` otherwise.
    pub(super) async fn link_local_node(
        self: &Arc<Self>,
        instance: &Instance,
        endpoint: &EndpointId,
    ) -> Result<bool> {
        let Some(node) = instance.local_node()? else {
            return Ok(false);
        };
        if node.endpoint != *endpoint {
            return Ok(false);
        }
        let Some(owner) = self.instance_owner().await? else {
            log::info!("the bundled node waits for the instance to have an owner");
            return Ok(false);
        };
        let Some(account) = self.store.account(&owner).await? else {
            return Ok(false);
        };
        let key = NewKey {
            endpoint: node.endpoint,
            name: &node.name,
            kind: DeviceKind::Desktop {},
        };
        let user = self.start_user(&account);
        match user.link_key(key).await? {
            LinkKey::Linked(_) => {
                log::info!("bundled node linked; user={owner} key={endpoint}");
                user.changed("");
                Ok(true)
            }
            LinkKey::Taken => Ok(false),
        }
    }
}
