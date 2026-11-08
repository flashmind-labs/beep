use super::*;
use crate::store::TestDb;
use iroh::SecretKey;

fn key(seed: u8) -> EndpointId {
    SecretKey::from_bytes(&[seed; 32]).public()
}

#[tokio::test]
async fn a_linked_key_signs_into_the_account_until_revoked() {
    let db = TestDb::new().await.expect("test database");
    let store = db.store().await.expect("test store");
    let phone = key(1);
    let desktop = key(2);
    let SignIn::Account {
        account, primary, ..
    } = store.sign_in(&phone).await.unwrap()
    else {
        panic!("a new key makes an account");
    };
    assert!(primary);
    assert_ne!(account.id, phone.to_string(), "an account id is its own");
    assert_eq!(account.external_id, account.id);
    let user = account.id.clone();
    let owner = UserKey::for_test(&user);
    let new = NewKey {
        endpoint: desktop,
        name: "MacBook",
        kind: DeviceKind::Desktop {},
    };
    assert!(matches!(
        store.link_key(&owner, new).await.unwrap(),
        LinkKey::Linked(_)
    ));
    assert_eq!(
        store.sign_in(&desktop).await.unwrap(),
        SignIn::Account {
            account: account.clone(),
            device: DeviceKind::Desktop {},
            primary: false,
        }
    );
    let keys = store.account_keys(&owner).await.unwrap();
    assert_eq!(keys.len(), 2);
    assert!(keys[0].primary);
    assert_eq!(keys[0].endpoint_id, phone.to_string());
    assert_eq!(
        store.live_keys(&user).await.unwrap(),
        [
            LiveKey {
                endpoint_id: phone.to_string(),
                kind: DeviceKind::Phone {},
            },
            LiveKey {
                endpoint_id: desktop.to_string(),
                kind: DeviceKind::Desktop {},
            },
        ]
    );

    // The phone's own key and a used key cannot be linked again.
    for endpoint in [phone, desktop] {
        let again = NewKey {
            endpoint,
            name: "Again",
            kind: DeviceKind::Desktop {},
        };
        assert_eq!(store.link_key(&owner, again).await.unwrap(), LinkKey::Taken);
    }

    // Nor can the primary key be removed.
    assert!(
        store
            .revoke_key(&owner, &phone.to_string())
            .await
            .unwrap()
            .is_none()
    );
    // A removed machine stops being one the agent can use.
    store
        .node_connected(
            &owner,
            crate::store::NewNode {
                endpoint: desktop,
                name: "desk",
                version: "0.1.0",
            },
        )
        .await
        .unwrap();
    store
        .set_project_sync(
            &owner,
            &beep_projects::imports::records::SyncRow {
                project: "game".into(),
                endpoint: desktop.to_string(),
                folder: "/srv/game".into(),
            },
        )
        .await
        .unwrap();
    assert!(
        store
            .revoke_key(&owner, &desktop.to_string())
            .await
            .unwrap()
            .is_some()
    );
    assert!(store.nodes(&owner).await.unwrap().is_empty());
    // Nor one that syncs projects.
    assert!(store.project_syncs(&owner).await.unwrap().is_empty());
    assert_eq!(store.sign_in(&desktop).await.unwrap(), SignIn::Revoked);
    assert_eq!(
        store.live_keys(&user).await.unwrap(),
        [LiveKey {
            endpoint_id: phone.to_string(),
            kind: DeviceKind::Phone {},
        }]
    );
    assert!(
        store
            .revoke_key(&owner, &desktop.to_string())
            .await
            .unwrap()
            .is_none()
    );
}
