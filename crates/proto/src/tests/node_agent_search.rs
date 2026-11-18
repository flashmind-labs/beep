use super::*;

#[test]
fn search_metadata_extends_the_released_wire_format() {
    let legacy_row = (
        5_u64,
        SessionId::from("saved"),
        Some("retry".to_owned()),
        90_u64,
    );
    let wire = rmp_serde::to_vec(&legacy_row).expect("legacy row encodes");
    let indexed: IndexRow = crate::lenient::from_slice(&wire).expect("legacy row decodes");
    assert!(indexed.metadata.is_none());

    let metadata = SearchSession {
        agent: "codex".into(),
        cwd: "/path/to/work".into(),
        title: None,
        started_at: 1,
        managed: None,
    };
    let newer = IndexRow {
        metadata: Some(metadata.clone()),
        ..indexed
    };
    let wire = rmp_serde::to_vec(&newer).expect("new row encodes");
    let legacy: (u64, SessionId, Option<String>, u64) =
        crate::lenient::from_slice(&wire).expect("legacy fields decode");
    assert_eq!(legacy, legacy_row);

    let legacy_hit = (SessionId::from("saved"), 90_u64, vec!["retry".to_owned()]);
    let wire = rmp_serde::to_vec(&legacy_hit).expect("legacy hit encodes");
    let hit: SearchHit = crate::lenient::from_slice(&wire).expect("legacy hit decodes");
    assert!(hit.metadata.is_none());

    let newer = SearchHit {
        metadata: Some(metadata),
        ..hit
    };
    let wire = rmp_serde::to_vec(&newer).expect("new hit encodes");
    let legacy: (SessionId, u64, Vec<String>) =
        crate::lenient::from_slice(&wire).expect("legacy fields decode");
    assert_eq!(legacy, legacy_hit);
}
