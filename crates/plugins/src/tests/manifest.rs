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
