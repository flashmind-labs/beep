use super::*;

fn var(name: &str, value: Option<&str>) -> HarnessEnv {
    HarnessEnv {
        name: name.into(),
        value: value.map(SecretText::new),
    }
}

#[test]
fn a_var_without_a_value_keeps_the_saved_one() {
    let kept = vec![
        EnvSecret {
            name: "HTTPS_PROXY".into(),
            value: SecretText::new("old"),
        },
        EnvSecret {
            name: "GONE".into(),
            value: SecretText::new("dropped"),
        },
    ];
    let merged = merged_env(
        kept,
        vec![
            var("HTTPS_PROXY", None),
            var("NEW", Some("fresh")),
            var("NEVER_SET", None),
        ],
    );
    let pairs: Vec<(&str, &str)> = merged
        .iter()
        .map(|var| (var.name.as_str(), var.value.expose()))
        .collect();
    assert_eq!(pairs, [("HTTPS_PROXY", "old"), ("NEW", "fresh")]);
}

#[tokio::test]
async fn saved_vars_that_do_not_open_stop_the_save() {
    let db = crate::store::TestDb::new().await.unwrap();
    let user = User::for_test(db.store().await.unwrap(), "alice");
    let key = CredentialKey::random();
    let saved = vec![EnvSecret {
        name: "HTTPS_PROXY".into(),
        value: SecretText::new("old"),
    }];
    user.save_harness_settings(
        &key,
        SaveHarness {
            machine_id: "m1",
            agent: "codex",
            enabled: true,
            program: "",
            home: "",
            args: &[],
            env: &saved,
        },
    )
    .await
    .unwrap();
    let wanted = || vec![var("HTTPS_PROXY", None)];
    let kept = env_to_save(&user, &key, "m1", "codex", wanted())
        .await
        .unwrap();
    assert_eq!(kept, saved);
    let other = CredentialKey::random();
    assert!(
        env_to_save(&user, &other, "m1", "codex", wanted())
            .await
            .is_err()
    );
}
