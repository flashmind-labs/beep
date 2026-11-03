use super::*;
use crate::store::{NewGitHubAccount, SignIn, TestDb, test_chat, test_turn};
use iroh::SecretKey;
use secrecy::{ExposeSecret, SecretString};

fn key(seed: u8) -> EndpointId {
    SecretKey::from_bytes(&[seed; 32]).public()
}

async fn account(store: &Store, endpoint: &EndpointId) -> String {
    match store.sign_in(endpoint).await.unwrap() {
        SignIn::Account { account, .. } => account.id,
        SignIn::Revoked => panic!("a new key makes an account"),
    }
}

#[tokio::test]
async fn a_phone_joins_a_computers_account_with_its_data() {
    let db = TestDb::new().await.expect("test database");
    let store = db.store().await.expect("test store");
    let credential_key = CredentialKey::random();
    let (mac, phone) = (key(11), key(12));
    let target = account(&store, &mac).await;
    let source = account(&store, &phone).await;
    let (target_key, source_key) = (UserKey::for_test(&target), UserKey::for_test(&source));
    let pool = &store.pool;

    // Both saved Home; only the phone saved Gym, with a routine at each.
    for (user, id, name) in [
        (&target, "t-home", "Home"),
        (&source, "s-home", "home"),
        (&source, "s-gym", "Gym"),
    ] {
        sqlx::query(
            "INSERT INTO places (user_id, id, name, latitude, longitude, radius)
             VALUES ($1, $2, $3, 1, 1, 100)",
        )
        .bind(user)
        .bind(id)
        .bind(name)
        .execute(pool)
        .await
        .unwrap();
    }
    for (id, schedule) in [
        ("c1", "arrive:s-home"),
        ("c2", "leave:s-gym:once"),
        ("c3", "0 9 * * *"),
    ] {
        sqlx::query(
            "INSERT INTO crons (user_id, id, label, prompt, schedule) VALUES ($1, $2, '', '', $3)",
        )
        .bind(&source)
        .bind(id)
        .bind(schedule)
        .execute(pool)
        .await
        .unwrap();
    }
    let chat = test_chat("phone chat");
    test_turn(&store, &source_key, chat, "from the phone", 1000)
        .await
        .unwrap();
    test_turn(
        &store,
        &target_key,
        test_chat("mac chat"),
        "from the mac",
        2000,
    )
    .await
    .unwrap();

    let invite = store
        .create_invite(&target_key, &mac, "MacBook", Duration::from_secs(600))
        .await
        .unwrap();
    let inviter = store.peek_invite(&invite.code).await.unwrap().unwrap();
    assert_eq!(inviter.account.id, target);
    assert_eq!(inviter.name, "MacBook");

    let into = MergeInto {
        code: &invite.code,
        target: &target,
        adopt_external_id: false,
        credential_key: &credential_key,
    };
    assert_eq!(
        store.merge_into(&target_key, into).await.unwrap(),
        Merge::SameAccount
    );
    let into = MergeInto {
        code: &invite.code,
        target: &target,
        adopt_external_id: true,
        credential_key: &credential_key,
    };
    let Merge::Merged {
        target: merged,
        keys,
    } = store.merge_into(&source_key, into).await.unwrap()
    else {
        panic!("the phone joins");
    };
    assert_eq!(merged, target);
    assert_eq!(keys, vec![phone.to_string()]);

    // The phone signs into the computer's account, as a linked key.
    assert_eq!(
        store.sign_in(&phone).await.unwrap(),
        SignIn::Account {
            account: Account {
                id: target.clone(),
                external_id: source.clone(),
            },
            device: DeviceKind::Phone {},
            primary: false,
        }
    );
    assert!(store.account(&source).await.unwrap().is_none());

    // The chat moved, and the target's log holds it.
    let members: Vec<String> =
        sqlx::query_scalar("SELECT user_id FROM chat_members WHERE chat_id=$1")
            .bind(chat)
            .fetch_all(pool)
            .await
            .unwrap();
    assert_eq!(members, vec![target.clone()]);
    let logged: i64 =
        sqlx::query_scalar("SELECT count(*) FROM updates WHERE user_id=$1 AND chat_id=$2")
            .bind(&target)
            .bind(chat)
            .fetch_one(pool)
            .await
            .unwrap();
    assert!(logged > 0);
    let page = store.updates_page(&target_key, 1).await.unwrap();
    assert!(page.reset, "clients of either account start over");
    assert_eq!(page.updates.len(), 6, "both chats, each with a turn");
    let again = store.updates_page(&target_key, page.last).await.unwrap();
    assert!(!again.reset && again.updates.is_empty());

    // Routines point at the target's Home and the moved Gym.
    let schedules: Vec<String> =
        sqlx::query_scalar("SELECT schedule FROM crons WHERE user_id=$1 ORDER BY id")
            .bind(&target)
            .fetch_all(pool)
            .await
            .unwrap();
    assert_eq!(
        schedules,
        ["arrive:t-home", "leave:s-gym:once", "0 9 * * *"]
    );
    let places: Vec<String> =
        sqlx::query_scalar("SELECT id FROM places WHERE user_id=$1 ORDER BY id")
            .bind(&target)
            .fetch_all(pool)
            .await
            .unwrap();
    assert_eq!(places, ["s-gym", "t-home"]);

    // The code works once.
    let into = MergeInto {
        code: &invite.code,
        target: &target,
        adopt_external_id: false,
        credential_key: &credential_key,
    };
    assert_eq!(
        store.merge_into(&source_key, into).await.unwrap(),
        Merge::NoInvite
    );
}

#[tokio::test]
async fn events_and_machines_move_with_the_account() {
    let db = TestDb::new().await.expect("test database");
    let store = db.store().await.expect("test store");
    let credential_key = CredentialKey::random();
    let (mac, phone, node) = (key(21), key(22), key(23));
    let target = account(&store, &mac).await;
    let source = account(&store, &phone).await;
    let target_key = UserKey::for_test(&target);
    let source_key = UserKey::for_test(&source);
    let pool = &store.pool;
    for (user, dedupe) in [(&source, "a"), (&source, "b"), (&target, "a")] {
        sqlx::query(
            "INSERT INTO app_events (user_id, app, kind, dedupe_key, summary, ids, raw, source, occurred_at)
             VALUES ($1, 'gmail', 'new_mail', $2, '', '{}', '{}', 'poll', now())",
        )
        .bind(user)
        .bind(dedupe)
        .execute(pool)
        .await
        .unwrap();
    }
    for query in [
        "INSERT INTO event_triage (user_id, decision, reason) VALUES ($1, 'ignore', '')",
        "INSERT INTO event_topics (user_id, topic, enabled) VALUES ($1, 'github_ci', false)",
        "INSERT INTO event_triggers (user_id, slug, account_id, trigger_id) VALUES ($1, 's', 'a', 't')",
        "INSERT INTO event_polls (user_id, source, polled_at) VALUES ($1, 'github', now())",
    ] {
        sqlx::query(query)
            .bind(&source)
            .execute(pool)
            .await
            .unwrap();
    }
    store
        .node_connected(
            &source_key,
            crate::store::NewNode {
                endpoint: node,
                name: "build box",
                version: "0.1.0",
            },
        )
        .await
        .unwrap();

    let token = SecretString::from("ghp_moved0000");
    store
        .save_github_account(
            &source_key,
            &credential_key,
            NewGitHubAccount {
                secret: &token,
                last4: "0000",
                login: "octo-one",
                orgs: &[],
                replace_default: false,
            },
        )
        .await
        .unwrap();

    // Both have a default GitHub account; the joined one keeps its own.
    let kept = SecretString::from("ghp_kept00001111");
    store
        .save_github_account(
            &target_key,
            &credential_key,
            NewGitHubAccount {
                secret: &kept,
                last4: "1111",
                login: "octo-two",
                orgs: &[],
                replace_default: false,
            },
        )
        .await
        .unwrap();

    let invite = store
        .create_invite(&target_key, &mac, "MacBook", Duration::from_secs(600))
        .await
        .unwrap();
    let into = MergeInto {
        code: &invite.code,
        target: &target,
        adopt_external_id: false,
        credential_key: &credential_key,
    };
    assert!(matches!(
        store.merge_into(&source_key, into).await.unwrap(),
        Merge::Merged { .. }
    ));

    for (table, rows) in [
        ("app_events", 2),
        ("event_triage", 1),
        ("event_topics", 1),
        ("event_triggers", 1),
        ("event_polls", 1),
        ("nodes", 1),
        ("credentials", 2),
    ] {
        let query = format!("SELECT count(*) FROM {table} WHERE user_id=$1");
        for (user, expected) in [(&source, 0), (&target, rows)] {
            let count: i64 = sqlx::query_scalar(&query)
                .bind(user)
                .fetch_one(pool)
                .await
                .unwrap();
            assert_eq!(count, expected, "{table} for {user}");
        }
    }
    // The GitHub token moved sealed for the account joined, and opens.
    let listed = store
        .credentials(&target_key, &credential_key)
        .await
        .unwrap();
    assert!(!listed[0].unreadable);
    let accounts = store
        .github_accounts(&target_key, &credential_key)
        .await
        .unwrap();
    let logins: Vec<(&str, bool)> = accounts
        .iter()
        .map(|a| (a.login.as_str(), a.default))
        .collect();
    assert_eq!(logins, [("octo-two", true), ("octo-one", false)]);
    let opened = store
        .machine_credentials(&target_key, &credential_key)
        .await
        .unwrap();
    assert_eq!(opened[1].secret.expose_secret(), "ghp_moved0000");
}

mod every_table;
