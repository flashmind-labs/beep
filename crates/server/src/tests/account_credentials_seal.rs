use super::*;

#[test]
fn a_sealed_value_opens_only_with_its_key_and_owner() {
    let server = SecretKey::from_bytes(&[5; 32]);
    let key = CredentialKey::derive(&server);
    let plain = SecretString::from("ghp_example0000");
    let owner = |user, slot, id| Owner { user, slot, id };
    let github = Slot::GitHub;
    let sealed = key.seal(&owner("alice", &github, "c1"), &plain).unwrap();
    assert!(!sealed.windows(4).any(|w| w == b"0000"));
    assert_eq!(
        key.open(&owner("alice", &github, "c1"), &sealed)
            .unwrap()
            .expose_secret(),
        "ghp_example0000"
    );
    // The same server key derives the same credentials key.
    let again = CredentialKey::derive(&server);
    assert!(again.open(&owner("alice", &github, "c1"), &sealed).is_ok());
    assert!(key.open(&owner("alice", &github, "c2"), &sealed).is_err());
    assert!(key.open(&owner("bob", &github, "c1"), &sealed).is_err());
    let forgejo = Slot::Forgejo("https://git.example.com".parse().unwrap());
    assert!(key.open(&owner("alice", &forgejo, "c1"), &sealed).is_err());
    let random = CredentialKey::random();
    assert!(
        random
            .open(&owner("alice", &github, "c1"), &sealed)
            .is_err()
    );
    assert!(
        key.open(&owner("alice", &github, "c1"), &sealed[..10])
            .is_err()
    );
    assert!(!format!("{key:?}").contains('['));
}

#[test]
fn harness_settings_open_only_for_their_machine_and_agent() {
    let key = CredentialKey::random();
    let owner = |machine, agent| HarnessOwner {
        user: "alice",
        machine,
        agent,
    };
    let plain = SecretString::from(r#"[{"name":"HTTPS_PROXY","value":"x"}]"#);
    let sealed = key.seal_harness(&owner("m1", "codex"), &plain).unwrap();
    let opened = key.open_harness(&owner("m1", "codex"), &sealed).unwrap();
    assert_eq!(opened.expose_secret(), plain.expose_secret());
    assert!(key.open_harness(&owner("m2", "codex"), &sealed).is_err());
    assert!(key.open_harness(&owner("m1", "claude"), &sealed).is_err());
}
