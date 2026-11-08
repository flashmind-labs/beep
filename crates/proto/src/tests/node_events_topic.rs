use super::*;

#[test]
fn topic_strings_preserve_hierarchy_and_literal_resource_identity() {
    let topic = Topic::parse("files.org.repo").unwrap();
    assert_eq!(topic.name(), "files");
    assert_eq!(topic.key(), Some("org.repo"));
    assert_eq!(topic.resource_segments(), ["org", "repo"]);
    assert_eq!(topic.to_string(), "files.org.repo");
    assert_ne!(topic, Topic::keyed("files", "org.repo"));
    assert!(!topic.matches(&Topic::keyed("files", "org.repo")));
    for key in ["/srv/a.b", "literal.*", "a%b", "workspace/café", "a\\b"] {
        let topic = Topic::keyed("files", key);
        assert_eq!(Topic::parse(&topic.to_string()).unwrap(), topic);
    }
    assert_eq!(
        Topic::keyed("files", "/srv/a.b").to_string(),
        "files.%2Fsrv%2Fa%2Eb"
    );
    assert_eq!(
        Topic::parse("files.%2fsrv%2fa%2eb").unwrap().to_string(),
        "files.%2Fsrv%2Fa%2Eb"
    );
    assert_eq!(
        Topic::new("plugin.example").to_string(),
        "plugin%2Eexample.*"
    );
}

#[test]
fn wildcard_subscriptions_and_broad_invalidations_match_at_component_boundaries() {
    let repo = Topic::parse("files.org.repo").unwrap();
    for pattern in ["*", "files", "files.*", "files.org.*", "files.org.repo"] {
        let subscription = Topic::parse(pattern).unwrap();
        assert!(subscription.matches(&repo), "{pattern}");
        assert!(repo.matches(&subscription), "broad invalidation: {pattern}");
    }
    for pattern in ["files.other.*", "files.org.other", "files.org", "reviews.*"] {
        assert!(!Topic::parse(pattern).unwrap().matches(&repo), "{pattern}");
    }
    assert!(Topic::parse("files.org.*").unwrap().covers(&repo));
    assert!(!repo.covers(&Topic::parse("files.org.*").unwrap()));
    assert!(Topic::keyed("files", "*").matches(&Topic::keyed("files", "*")));
    assert!(!Topic::keyed("files", "*").matches(&repo));
}

#[test]
fn malformed_strings_stay_invalid_through_wire_decode_for_correlated_errors() {
    for text in [
        "",
        ".",
        "files..repo",
        "files.*.repo",
        "files.%",
        "files.%GG",
        "files.%FF",
        "files.%00",
        "*.repo",
        "%2A",
        "%2A.*",
        "files/raw",
    ] {
        assert!(Topic::parse(text).is_err(), "{text}");
        let bytes = rmp_serde::to_vec(text).unwrap();
        let decoded: Topic = crate::lenient::from_slice(&bytes).unwrap();
        assert!(!decoded.valid(), "{text}");
    }
    assert!(Topic::parse(&"a".repeat(MAX_TOPIC_BYTES + 1)).is_err());
    assert!(!Topic::keyed("files", "a".repeat(4097)).valid());
}

#[test]
fn compact_wire_contains_a_string_without_struct_field_names() {
    let topic = Topic::parse("files.org.repo").unwrap();
    assert_eq!(
        rmp_serde::to_vec(&topic).unwrap(),
        rmp_serde::to_vec("files.org.repo").unwrap()
    );
    assert_eq!(
        crate::lenient::from_slice::<Topic>(&rmp_serde::to_vec(&topic).unwrap()).unwrap(),
        topic
    );
}
