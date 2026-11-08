use super::*;

fn png(name: &str, len: usize) -> Upload {
    let mut data = b"\x89PNG\r\n\x1a\n".to_vec();
    data.resize(len.max(data.len()), 0);
    Upload {
        name: name.into(),
        mime: "image/png".into(),
        data,
    }
}

#[test]
fn the_node_refuses_images_the_server_would() {
    let text = || "look".to_owned();
    let ok = Prompt::with_images(text(), vec![png("a.png", 100)]).unwrap();
    assert_eq!(ok.images.len(), 1);

    let many = (0..=beep_proto::MAX_AGENT_IMAGES)
        .map(|i| png(&format!("{i}.png"), 10))
        .collect();
    let refused = Prompt::with_images(text(), many).unwrap_err();
    assert!(refused.starts_with("At most"), "{refused}");

    let big = png("big.png", beep_proto::MAX_AGENT_IMAGE + 1);
    let refused = Prompt::with_images(text(), vec![big]).unwrap_err();
    assert!(refused.starts_with("big.png is over"), "{refused}");

    let mut fake = png("fake.png", 10);
    fake.mime = "image/jpeg".into();
    let refused = Prompt::with_images(text(), vec![fake]).unwrap_err();
    assert!(refused.contains("not the image/jpeg"), "{refused}");

    let mut svg = png("x.svg", 10);
    svg.mime = "image/svg+xml".into();
    assert!(Prompt::with_images(text(), vec![svg]).is_err());
}

#[test]
fn steering_requires_advertising_and_a_version_with_the_idle_fallback() {
    for (name, version, supported, expected) in [
        ("@agentclientprotocol/codex-acp", "2.4.0", true, true),
        ("@agentclientprotocol/codex-acp", "2.3.0", true, false),
        (
            "@agentclientprotocol/claude-agent-acp",
            "0.65.0",
            true,
            true,
        ),
        (
            "@agentclientprotocol/claude-agent-acp",
            "0.64.0",
            true,
            false,
        ),
        ("@agentclientprotocol/codex-acp", "2.4.0", false, false),
        (
            "@agentclientprotocol/codex-acp",
            "2.4.0-beta.1",
            true,
            false,
        ),
        ("other-agent", "3.0.0", true, false),
        ("flsh", "0.2.13", true, false),
        ("flsh", "0.2.14", true, true),
        ("flsh", "0.2.14", false, false),
    ] {
        let initialized = serde_json::from_value(json!({
            "protocolVersion": 1,
            "agentCapabilities": {},
            "agentInfo": {"name": name, "version": version},
            "_meta": {"steering": {"supported": supported}},
        }))
        .unwrap();
        assert_eq!(takes_steering(&initialized), expected, "{name} {version}");
    }
}
