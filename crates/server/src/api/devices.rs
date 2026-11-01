//! Push registrations, linked devices and joining an account.

use super::{Reply, found};
use crate::{
    nodes::push_sync,
    protocol::*,
    push::CLOSE_REVOKED,
    router::Cx,
    store::{AccountKey, Inviter, LinkKey, Merge, MergeInto, NewKey, PushDevice, PushService},
};
use iroh::EndpointId;
use std::time::{Duration, SystemTime};
use tokio::time::sleep;

/// How long a join code works.
const INVITE_TTL: Duration = Duration::from_secs(10 * 60);
/// How long the joining keys keep their connections, so the reply that
/// they joined gets there first.
const REJOIN_AFTER: Duration = Duration::from_secs(1);

pub(super) async fn register_device(cx: Cx, req: RegisterDevice) -> Reply<()> {
    let service = match req.apns {
        None => PushService::Fcm,
        Some(ApnsEnvironment::Sandbox {}) => PushService::ApnsSandbox,
        Some(ApnsEnvironment::Production {}) => PushService::ApnsProduction,
    };
    let device = PushDevice {
        endpoint: cx.connection.remote_id(),
        device_id: &req.device_id,
        token: &req.fcm_token,
        service,
    };
    cx.user.register_device(device).await?;
    Ok(())
}

pub(super) async fn unregister_device(cx: Cx, req: UnregisterDevice) -> Reply<()> {
    found(cx.user.unregister_device(&req.device_id).await?)
}

/// Links a computer whose offer the caller scanned. Any key of the account
/// may link others.
pub(super) async fn link_device(cx: Cx, LinkDevice { offer }: LinkDevice) -> Reply<LinkedDevice> {
    let server = cx.pusher.server_id();
    offer
        .verify(server, SystemTime::now())
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    // The server's own code verifies too; it names the server, not a device.
    if offer.device == server {
        return Err(ApiError::bad_request(
            "that is this server's own code, not a device's",
        ));
    }
    let key = NewKey {
        endpoint: offer.device,
        name: offer.name.trim(),
        kind: DeviceKind::Desktop {},
    };
    match cx.user.link_key(key).await? {
        LinkKey::Linked(key) => {
            // Also the new device's first notice, which tells it it is linked.
            cx.user.changed("");
            Ok(linked_device(&cx, key))
        }
        LinkKey::Taken => Err(ApiError::Conflict {
            message: "that device is already linked to an account".into(),
        }),
    }
}

pub(super) async fn list_devices(cx: Cx, _: ListDevices) -> Reply<Vec<LinkedDevice>> {
    let keys = cx.user.account_keys().await?;
    Ok(keys
        .into_iter()
        .map(|key| linked_device(&cx, key))
        .collect())
}

/// The primary key removes any linked one; a linked key only itself.
pub(super) async fn remove_device(cx: Cx, RemoveDevice { id }: RemoveDevice) -> Reply<()> {
    let caller = cx.connection.remote_id();
    if !cx.primary && id != caller.to_string() {
        return Err(ApiError::Unauthorized {});
    }
    let key: EndpointId = id
        .parse()
        .map_err(|_| ApiError::bad_request("not a device id"))?;
    found(cx.user.revoke_key(&id).await?)?;
    cx.user.changed("");
    cx.pusher.forget(key);
    cx.app.nodes.forget(&key, CLOSE_REVOKED);
    // The machines that synced projects with it stop pulling from it.
    if let Err(e) = push_sync(&cx.app.nodes, &cx.user, None).await {
        log::warn!("sync lists not sent after a removed device: {e:#}");
    }
    Ok(())
}

/// A code another device joins the caller's account with. Any key of the
/// account may show one, as it already sees everything a joiner would.
pub(super) async fn invite_device(
    cx: Cx,
    InviteDevice { name }: InviteDevice,
) -> Reply<DeviceInvite> {
    let name = name.trim();
    if name.is_empty() || name.len() > link::MAX_NAME {
        return Err(ApiError::bad_request(
            "the device name is empty or too long",
        ));
    }
    let invite = cx
        .user
        .create_invite(&cx.connection.remote_id(), name, INVITE_TTL)
        .await?;
    Ok(DeviceInvite {
        uri: link::join_uri(&invite.code),
        code: invite.code,
        expires_at: invite.expires_at.into(),
    })
}

pub(super) async fn peek_invite(cx: Cx, PeekInvite { code }: PeekInvite) -> Reply<JoinPreview> {
    let inviter = inviter(&cx, &code).await?;
    Ok(JoinPreview {
        name: inviter.name,
        kind: inviter.kind,
    })
}

/// The account behind a live code other than the caller's own.
async fn inviter(cx: &Cx, code: &str) -> Result<Inviter, ApiError> {
    let code = link::join_code(code).ok_or_else(|| ApiError::bad_request("not a join code"))?;
    let inviter = cx
        .app
        .store
        .peek_invite(&code)
        .await?
        .ok_or(ApiError::NotFound {})?;
    if inviter.account.id == cx.user.id() {
        return Err(ApiError::Conflict {
            message: "that code is for this account".into(),
        });
    }
    Ok(inviter)
}

/// Moves the caller's account into the one that made `code`, then closes
/// the connections of its keys so they sign in as the account joined.
pub(super) async fn join_account(cx: Cx, JoinAccount { code }: JoinAccount) -> Reply<()> {
    if !cx.primary {
        return Err(ApiError::Unauthorized {});
    }
    let inviter = inviter(&cx, &code).await?;
    let target = cx.app.user_for(&inviter.account);
    // The apps connected under the joining account stay connected when the
    // account joined has none of its own.
    let (mine, theirs) = tokio::join!(cx.user.composio_connected(), target.composio_connected());
    let into = MergeInto {
        code: &link::join_code(&code).unwrap_or_default(),
        target: target.id(),
        adopt_external_id: mine == Some(true) && theirs == Some(false),
        credential_key: &cx.app.credential_key,
    };
    match cx.user.merge_into(into).await? {
        Merge::Merged { target, keys } => {
            log::info!("account merged; from={} into={target}", cx.user.id());
            cx.app.merged(cx.user.id(), &target).await?;
            if let Some(user) = cx.app.user(&target) {
                user.changed("");
            }
            // A machine signs in again now, as the account it joined.
            for key in keys.iter().filter_map(|key| key.parse::<EndpointId>().ok()) {
                cx.app.nodes.forget(&key, 0);
            }
            let pusher = cx.pusher.clone();
            tokio::spawn(async move {
                sleep(REJOIN_AFTER).await;
                for key in keys.iter().filter_map(|key| key.parse().ok()) {
                    pusher.reconnect(key);
                }
            });
            Ok(())
        }
        Merge::NoInvite => Err(ApiError::NotFound {}),
        Merge::SameAccount => Err(ApiError::Conflict {
            message: "that code is for this account".into(),
        }),
        Merge::Busy => Err(ApiError::Conflict {
            message: "a chat is still running; wait for it or stop it, then join".into(),
        }),
    }
}

fn linked_device(cx: &Cx, key: AccountKey) -> LinkedDevice {
    LinkedDevice {
        this_device: key.endpoint_id == cx.connection.remote_id().to_string(),
        id: key.endpoint_id,
        name: key.name,
        kind: key.kind,
        primary: key.primary,
        linked_at: key.linked_at.into(),
    }
}
