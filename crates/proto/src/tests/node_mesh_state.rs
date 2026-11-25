use super::*;

#[test]
fn project_names_and_remote_renames_preserve_positional_compatibility() {
    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct ReleasedRecord {
        url: String,
        syncthing_files: Option<bool>,
        git_auto_recovery: bool,
    }
    let old = ReleasedRecord {
        url: "https://github.com/team/old".into(),
        syncthing_files: Some(false),
        git_auto_recovery: true,
    };
    let decoded = ProjectRecord::from_bytes(&rmp_serde::to_vec(&old).unwrap()).unwrap();
    assert_eq!(decoded.display_name, None);
    assert!(decoded.remote_renames.is_empty());
    let current = ProjectRecord {
        display_name: Some("New name".into()),
        remote_renames: vec![RemoteRename {
            from: old.url.clone(),
            to: "https://github.com/team/new".into(),
        }],
        ..decoded
    };
    let bytes = current.to_bytes().unwrap();
    assert_eq!(
        crate::lenient::from_slice::<ReleasedRecord>(&bytes).unwrap(),
        old
    );
    assert_eq!(ProjectRecord::from_bytes(&bytes).unwrap(), current);
}

#[test]
fn dictation_settings_preserve_positional_compatibility() {
    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct ReleasedDictation {
        enabled: bool,
    }
    assert_eq!(
        DictationSettings::from_bytes(&rmp_serde::to_vec(&[(); 0]).unwrap()).unwrap(),
        DictationSettings::default(),
    );
    let old = ReleasedDictation { enabled: true };
    assert_eq!(
        DictationSettings::from_bytes(&rmp_serde::to_vec(&old).unwrap()).unwrap(),
        DictationSettings {
            enabled: true,
            model: None,
        },
    );
    let saved = DictationSettings {
        enabled: true,
        model: Some("elevenlabs:scribe_v3_live".into()),
    };
    let bytes = saved.to_bytes().unwrap();
    assert_eq!(
        crate::lenient::from_slice::<ReleasedDictation>(&bytes).unwrap(),
        old,
    );
    assert_eq!(DictationSettings::from_bytes(&bytes).unwrap(), saved);
}
