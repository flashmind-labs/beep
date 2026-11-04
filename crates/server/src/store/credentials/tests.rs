use super::*;
use crate::store::test_db::TestDb;
use secrecy::ExposeSecret;

#[tokio::test]
async fn a_secret_is_sealed_in_its_row_and_opens_back() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let alice = UserKey::for_test("alice");
    let key = CredentialKey::random();
    let secret = SecretString::from("ghp_plaintextvalue9f3c");
    let saved = store
        .save_github_account(
            &alice,
            &key,
            NewGitHubAccount {
                secret: &secret,
                last4: "9f3c",
                login: "octocat",
                orgs: &[],
                replace_default: false,
            },
        )
        .await
        .unwrap();
    assert_eq!(saved.login, "octocat");
    assert!(saved.default);
    let listed = store.credentials(&alice, &key).await.unwrap();
    assert_eq!(listed[0].kind, CredentialKind::GitHub {});
    assert_eq!(listed[0].env_var, "GH_TOKEN");
    assert_eq!(listed[0].account.as_deref(), Some("octocat"));

    let (sealed, env_var): (Vec<u8>, String) =
        sqlx::query_as("SELECT secret, env_var FROM credentials WHERE id=$1")
            .bind(&saved.id)
            .fetch_one(&store.pool)
            .await
            .unwrap();
    assert_eq!(env_var, "GH_TOKEN");
    assert!(
        !sealed
            .windows(b"plaintext".len())
            .any(|w| w == b"plaintext")
    );
    let opened = store.machine_credentials(&alice, &key).await.unwrap();
    assert_eq!(opened.len(), 1);
    assert_eq!(opened[0].secret.expose_secret(), "ghp_plaintextvalue9f3c");
    assert!(!store.credentials(&alice, &key).await.unwrap()[0].unreadable);
    // Another key opens nothing; the row is skipped, not an error, and
    // listed as unreadable.
    let other = CredentialKey::random();
    assert!(store.credentials(&alice, &other).await.unwrap()[0].unreadable);
    assert!(
        store
            .machine_credentials(&alice, &other)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn several_github_accounts_one_default() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let alice = UserKey::for_test("alice");
    let key = CredentialKey::random();
    let save = |token: &'static str, login: &'static str, orgs: Vec<String>, replace_default| {
        let store = store.clone();
        let alice = alice.clone();
        let key = &key;
        async move {
            let secret = SecretString::from(token);
            store
                .save_github_account(
                    &alice,
                    key,
                    NewGitHubAccount {
                        secret: &secret,
                        last4: "0000",
                        login,
                        orgs: &orgs,
                        replace_default,
                    },
                )
                .await
                .unwrap()
        }
    };
    let one = save("tok-one", "octo-one", vec!["acme".into()], false).await;
    let two = save("tok-two", "octo-two", vec!["widget-co".into()], false).await;
    assert!(one.default);
    assert!(!two.default);
    // The same login again replaces it, and it stays the default.
    let again = save("tok-one-b", "Octo-One", vec![], false).await;
    assert!(again.default);
    assert_ne!(again.id, one.id);
    let accounts = store.github_accounts(&alice, &key).await.unwrap();
    let logins: Vec<(&str, bool)> = accounts
        .iter()
        .map(|a| (a.login.as_str(), a.default))
        .collect();
    assert_eq!(logins, [("Octo-One", true), ("octo-two", false)]);
    assert_eq!(accounts[1].orgs, ["widget-co"]);

    // Apps from before several accounts see the default alone.
    let listed = store.credentials(&alice, &key).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, again.id);

    // Machines get the default as GH_TOKEN and the other by owner.
    let opened = store.machine_credentials(&alice, &key).await.unwrap();
    let env = crate::account::credentials::for_machines(opened).env;
    let env: Vec<(&str, &str)> = env
        .iter()
        .map(|var| (var.name.as_str(), var.value.expose()))
        .collect();
    assert_eq!(
        env,
        [
            ("BEEP_GH_TOKEN_OCTO_TWO", "tok-two"),
            ("BEEP_GH_TOKEN_WIDGET_CO", "tok-two"),
            ("GH_TOKEN", "tok-one-b"),
        ]
    );

    assert!(store.set_default_github(&alice, &two.id).await.unwrap());
    assert!(!store.set_default_github(&alice, "nope").await.unwrap());
    let accounts = store.github_accounts(&alice, &key).await.unwrap();
    assert_eq!(accounts[0].id, two.id);
    assert!(accounts[0].default && !accounts[1].default);

    // A token for another login from an older app replaces the default.
    let three = save("tok-three", "octo-three", vec![], true).await;
    assert!(three.default);
    let accounts = store.github_accounts(&alice, &key).await.unwrap();
    let logins: Vec<&str> = accounts.iter().map(|a| a.login.as_str()).collect();
    assert_eq!(logins, ["octo-three", "Octo-One"]);

    // Removing the default makes the oldest left the default.
    assert!(store.remove_credential(&alice, &three.id).await.unwrap());
    let accounts = store.github_accounts(&alice, &key).await.unwrap();
    assert_eq!(accounts.len(), 1);
    assert!(accounts[0].default);
    let flagged: bool = sqlx::query_scalar("SELECT is_default FROM credentials WHERE id=$1")
        .bind(&accounts[0].id)
        .fetch_one(&store.pool)
        .await
        .unwrap();
    assert!(flagged);
}

#[tokio::test]
async fn credentials_belong_to_their_owner() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let alice = UserKey::for_test("alice");
    let bob = UserKey::for_test("bob");
    let key = CredentialKey::random();
    let forgejo = Slot::Forgejo("https://git.example.com".parse().unwrap());
    let save = |slot: &Slot, secret: &str, to_machines: bool| {
        let secret = SecretString::from(secret);
        let slot = slot.clone();
        let store = store.clone();
        let alice = alice.clone();
        let key = &key;
        async move {
            store
                .save_credential(
                    &alice,
                    key,
                    NewCredential {
                        slot: &slot,
                        secret: &secret,
                        last4: "0000",
                        account: None,
                        to_machines,
                    },
                )
                .await
                .unwrap()
        }
    };
    let first = save(&forgejo, "token-one", true).await;
    let second = save(
        &Slot::Forgejo("https://code.example.org".parse().unwrap()),
        "token-two",
        true,
    )
    .await;
    save(&Slot::ApiKey(ApiProvider::OpenAi), "sk-kept-here", false).await;
    // Saving the same server again replaces it.
    let replaced = save(&forgejo, "token-three", true).await;
    assert_ne!(replaced.id, first.id);

    assert!(store.credentials(&bob, &key).await.unwrap().is_empty());
    assert!(!store.remove_credential(&bob, &second.id).await.unwrap());
    assert!(
        store
            .machine_credentials(&bob, &key)
            .await
            .unwrap()
            .is_empty()
    );

    let listed = store.credentials(&alice, &key).await.unwrap();
    assert_eq!(listed.len(), 3);
    // Only those marked for machines go to them.
    let opened = store.machine_credentials(&alice, &key).await.unwrap();
    let secrets: Vec<&str> = opened.iter().map(|o| o.secret.expose_secret()).collect();
    assert_eq!(secrets, ["token-two", "token-three"]);
    // The git hosts open for the overview, API keys never.
    let git = store.git_logins(&alice, &key).await.unwrap();
    let hosts: Vec<(&str, &str)> = git
        .iter()
        .map(|login| {
            let host = match &login.slot {
                Slot::Forgejo(url) => url.as_str(),
                _ => "not a forgejo server",
            };
            (host, login.secret.expose_secret())
        })
        .collect();
    assert_eq!(
        hosts,
        [
            ("https://code.example.org", "token-two"),
            ("https://git.example.com", "token-three")
        ]
    );
    assert!(store.git_logins(&bob, &key).await.unwrap().is_empty());

    assert!(store.remove_credential(&alice, &second.id).await.unwrap());
    assert_eq!(store.credentials(&alice, &key).await.unwrap().len(), 2);

    // A row moved to another user, as by a write that skipped the
    // store, does not open for them.
    sqlx::query("UPDATE credentials SET user_id=$1 WHERE id=$2")
        .bind(bob.as_str())
        .bind(&replaced.id)
        .execute(&store.pool)
        .await
        .unwrap();
    let moved = store.credentials(&bob, &key).await.unwrap();
    assert_eq!(moved.len(), 1);
    assert!(moved[0].unreadable);

    // A provider this server does not know, as a newer one saved it,
    // lists as unreadable without failing the rest.
    sqlx::query("UPDATE credentials SET provider='newai' WHERE user_id=$1 AND kind='api_key'")
        .bind(alice.as_str())
        .execute(&store.pool)
        .await
        .unwrap();
    let listed = store.credentials(&alice, &key).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(
        listed[0].kind,
        CredentialKind::ApiKey {
            provider: "newai".into()
        }
    );
    assert!(listed[0].unreadable);
    assert!(
        store
            .machine_credentials(&bob, &key)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn agent_tokens_go_to_machines_but_not_to_the_credentials_list() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let alice = UserKey::for_test("alice");
    let key = CredentialKey::random();
    let secret = SecretString::from("sk-ant-oat01-plaintextvalue7d1e");
    let claude = Slot::AgentToken(AgentAccount::Claude);
    store
        .save_credential(
            &alice,
            &key,
            NewCredential {
                slot: &claude,
                secret: &secret,
                last4: "7d1e",
                account: None,
                to_machines: true,
            },
        )
        .await
        .unwrap();
    // ListCredentials stays as older apps know it.
    assert!(store.credentials(&alice, &key).await.unwrap().is_empty());
    let tokens = store.agent_tokens(&alice, &key).await.unwrap();
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].agent, AgentAccount::Claude);
    assert_eq!(tokens[0].last4, "7d1e");
    assert!(!tokens[0].unreadable);
    let opened = store.machine_credentials(&alice, &key).await.unwrap();
    assert_eq!(opened.len(), 1);
    assert_eq!(opened[0].slot, claude);
    assert_eq!(
        opened[0].secret.expose_secret(),
        "sk-ant-oat01-plaintextvalue7d1e"
    );
    let other_key = CredentialKey::random();
    assert!(store.agent_tokens(&alice, &other_key).await.unwrap()[0].unreadable);

    assert!(
        !store
            .remove_agent_token(&alice, AgentAccount::Codex)
            .await
            .unwrap()
    );
    assert!(
        store
            .remove_agent_token(&alice, AgentAccount::Claude)
            .await
            .unwrap()
    );
    assert!(store.agent_tokens(&alice, &key).await.unwrap().is_empty());
    assert!(
        store
            .machine_credentials(&alice, &key)
            .await
            .unwrap()
            .is_empty()
    );
}
