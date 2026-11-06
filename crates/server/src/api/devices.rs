//! Push registrations, linked devices and joining an account.

use super::{Reply, found};
use crate::{
    account::{guest, login},
    api::router::Cx,
    nodes::push_sync,
    projects::imports::machine_removed,
    protocol::*,
    push::CLOSE_REVOKED,
    store::{AccountKey, Inviter, LinkKey, Merge, MergeInto, NewKey, PushDevice, PushService},
};
use iroh::EndpointId;
use std::time::{Duration, SystemTime};
use tokio::time::sleep;

/// How long a join code works.
const INVITE_TTL: Duration = Duration::from_secs(10 * 60);
/// How long joining keys keep their events streams, so the reply gets there first.
const REJOIN_AFTER: Duration = Duration::from_secs(1);

pub(super) async fn register_device(cx: Cx, req: RegisterDevice) -> Reply<()> {
    let service = match req.apns {
        None => PushService::Fcm,
        Some(ApnsEnvironment::Sandbox {}) => PushService::ApnsSandbox,
        Some(ApnsEnvironment::Production {}) => PushService::ApnsProduction,
    };
    let device = PushDevice {
        endpoint: cx.key,
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

/// Links a computer whose offer the caller scanned.
pub(super) async fn link_device(cx: Cx, LinkDevice { offer }: LinkDevice) -> Reply<LinkedDevice> {
    let server = cx.server;
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
    let caller = cx.key;
    if !cx.primary && id != caller.to_string() {
        return Err(ApiError::Unauthorized {});
    }
    let key: EndpointId = id
        .parse()
        .map_err(|_| ApiError::bad_request("not a device id"))?;
    unlink(&cx, key).await
}

/// Revokes one of the account's keys and what hangs off it; `NotFound` for the primary key.
pub(super) async fn unlink(cx: &Cx, key: EndpointId) -> Reply<()> {
    let removed = cx
        .user
        .revoke_key(&key.to_string())
        .await?
        .ok_or(ApiError::NotFound {})?;
    cx.user.changed("");
    if let Err(e) = cx.app.store.end_streams(&key).await {
        log::warn!("the streams of a removed device were not ended: {e:#}");
    }
    cx.app.nodes.forget(&key, CLOSE_REVOKED);
    machine_removed(&cx.app, &cx.user, &removed).await;
    // The machines that synced projects with it stop pulling from it.
    if let Err(e) = push_sync(&cx.app.nodes, &cx.user, None).await {
        log::warn!("sync lists not sent after a removed device: {e:#}");
    }
    Ok(())
}

/// A code another device joins the caller's account with.
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
    let invite = cx.user.create_invite(&cx.key, name, INVITE_TTL).await?;
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

/// A website address for attaching a login to the caller's account.
pub(super) async fn start_login(cx: Cx, StartLogin { device }: StartLogin) -> Reply<SecretText> {
    login::start(&cx.app, &cx.key, &device).await
}

/// Attaches the login in `token` to the caller's account.
pub(super) async fn complete_login(cx: Cx, CompleteLogin { token }: CompleteLogin) -> Reply<()> {
    let key = guest::new_key(cx.key, cx.platform);
    login::complete(&cx.app, key, token.expose()).await
}

/// Moves the caller's account into the one that made `code`, then reconnects its keys.
pub(super) async fn join_account(cx: Cx, JoinAccount { code }: JoinAccount) -> Reply<()> {
    if !cx.primary {
        return Err(ApiError::Unauthorized {});
    }
    let inviter = inviter(&cx, &code).await?;
    let target = cx.app.user_for(&inviter.account);
    // The joiner's apps stay connected when the target has none of its own.
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
            // Its apps open their events streams again, as the account it joined.
            let store = cx.app.store.clone();
            tokio::spawn(async move {
                sleep(REJOIN_AFTER).await;
                for key in keys.iter().filter_map(|key| key.parse::<EndpointId>().ok()) {
                    if let Err(e) = store.end_streams(&key).await {
                        log::warn!("the streams of a moved device were not ended: {e:#}");
                    }
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
        this_device: key.endpoint_id == cx.key.to_string(),
        id: key.endpoint_id,
        name: key.name,
        kind: key.kind,
        primary: key.primary,
        linked_at: key.linked_at.into(),
    }
}
