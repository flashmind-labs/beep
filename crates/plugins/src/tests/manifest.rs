use super::*;

const VALID: &str = r#"{ "id": "composer-cat", "name": "Composer cat", "version": "0.1.0",
  "description": "A cat.", "entry": "main.ts", "permissions": ["ui.add"] }"#;

#[test]
fn parses_a_full_manifest() {
    let manifest = Manifest::parse(VALID).unwrap();

    assert_eq!(manifest.id.to_string(), "composer-cat");
    assert_eq!(manifest.version, semver::Version::new(0, 1, 0));
    assert_eq!(manifest.permissions, [Permission::UiAdd]);
    assert!(manifest.allows(Permission::UiAdd));
}

#[test]
fn applies_defaults() {
    let manifest = Manifest::parse(r#"{ "id": "ab", "name": "n", "version": "1.2.3" }"#).unwrap();

    assert_eq!(manifest.description, "");
    assert_eq!(manifest.entry, PathBuf::from("main.ts"));
    assert!(manifest.permissions.is_empty());
}

#[test]
fn rejects_bad_ids() {
    for id in ["a", "1ab", "Ab", "a_b", "-ab", "", &"a".repeat(41)] {
        assert!(id.parse::<PluginId>().is_err(), "{id:?} should be refused");
    }
    for id in ["ab", "a1", "a-b-c", &"a".repeat(40)] {
        assert!(id.parse::<PluginId>().is_ok(), "{id:?} should be accepted");
    }
}

#[test]
fn rejects_unknown_permissions_and_fields() {
    let permission = VALID.replace("ui.add", "fs.read");
    assert!(Manifest::parse(&permission).is_err());

    let field = VALID.replace("\"name\"", "\"extra\": 1, \"name\"");
    assert!(Manifest::parse(&field).is_err());
}

#[test]
fn rejects_bad_version_and_entry() {
    assert!(Manifest::parse(&VALID.replace("0.1.0", "one")).is_err());

    for entry in ["../x.ts", "/abs.ts", ""] {
        let text = VALID.replace("main.ts", entry);
        assert!(
            matches!(Manifest::parse(&text), Err(ManifestError::Entry(_))),
            "{entry:?}"
        );
    }
}

#[test]
fn id_must_match_folder() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("other-name");
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join(MANIFEST_FILE), VALID).unwrap();

    assert!(matches!(
        Manifest::read(&dir),
        Err(ManifestError::IdMismatch { .. })
    ));

    let dir = root.path().join("composer-cat");
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join(MANIFEST_FILE), VALID).unwrap();
    assert!(Manifest::read(&dir).is_ok());
}

#[test]
fn parses_the_sessions_permission() {
    let manifest =
        Manifest::parse(&VALID.replace("\"ui.add\"", "\"ui.add\", \"sessions.read\"")).unwrap();

    assert!(manifest.allows(Permission::SessionsRead));
    assert_eq!(Permission::SessionsRead.to_string(), "sessions.read");
}

#[test]
fn activation_defaults_to_startup() {
    let manifest = Manifest::parse(VALID).unwrap();

    assert_eq!(manifest.activation, [Activation::Startup]);
    assert!(manifest.starts_at_launch());
}

#[test]
fn activation_names_slots_and_local_topics() {
    let text = VALID.replace(
        "\"permissions\"",
        "\"activation\": [\"slot:composer\", \"topic:ui.composer\"], \"permissions\"",
    );

    let manifest = Manifest::parse(&text).unwrap();

    assert_eq!(
        manifest.activation,
        [
            Activation::Slot(Slot::Composer),
            Activation::Topic(Topic::local("ui.composer").unwrap())
        ]
    );
    assert!(!manifest.starts_at_launch());
    assert_eq!(manifest.activation[1].to_string(), "topic:ui.composer");
}

#[test]
fn bad_activations_fail_the_manifest() {
    for bad in [
        "\"slot:sidebar\"",
        "\"topic:agent\"",
        "\"topic:\"",
        "\"later\"",
        "3",
    ] {
        let text = VALID.replace(
            "\"permissions\"",
            &format!("\"activation\": [{bad}], \"permissions\""),
        );
        assert!(Manifest::parse(&text).is_err(), "{bad}");
    }

    let empty = VALID.replace("\"permissions\"", "\"activation\": [], \"permissions\"");
    assert!(matches!(
        Manifest::parse(&empty),
        Err(ManifestError::NoActivation)
    ));
}
