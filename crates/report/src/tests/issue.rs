use super::*;
use crate::period::Period;
use beep_mesh::DeviceKind;
use iroh::SecretKey;

fn machine(name: &str, answer: Answer) -> MachineEntry {
    MachineEntry {
        id: SecretKey::generate().public(),
        name: name.into(),
        kind: DeviceKind::Node,
        answer,
        version: "0.4.1".into(),
        os: "linux".into(),
        os_version: "6.8".into(),
    }
}

fn manifest(text: &str) -> Manifest {
    Manifest {
        version: 1,
        sender: SecretKey::generate().public(),
        mesh: None,
        app: "desktop".into(),
        app_version: "0.2.0".into(),
        text: text.into(),
        period: Period::Today,
        since: 0,
        made_at: 0,
        machines: vec![
            machine("build-box", Answer::Answered),
            machine(
                "gpu-box",
                Answer::Failed {
                    reason: "/path/to/work".into(),
                },
            ),
            machine("old-box", Answer::Silent),
        ],
        files: Vec::new(),
    }
}

#[test]
fn the_issue_names_no_machine_path_or_log() {
    let m = manifest("sync stalls\nsecond line cc @someone");
    let got = issue(&m, "R-1234").expect("redaction rules build");
    let sender = m.sender.to_string();
    assert_eq!(got.title, "Problem report R-1234");
    assert!(got.body.contains("R-1234"));
    assert!(got.body.contains("node, linux 6.8, 0.4.1: answered"));
    assert!(got.body.contains("did not answer"));
    assert!(got.body.contains("failed"));
    assert!(got.body.contains(&sender[..8]));
    assert!(got.body.contains("> sync stalls\n> second line"));
    assert!(got.body.contains("`@someone`"));
    for secret in ["build-box", "gpu-box", "old-box", "/path/to/work", &sender] {
        assert!(!got.body.contains(secret), "{secret} leaked");
    }
}

#[test]
fn the_text_is_redacted_here() {
    let got =
        issue(&manifest("token=abcdef123456789012345678"), "R-1").expect("redaction rules build");
    assert!(!got.body.contains("abcdef123456789012345678"));
}

#[test]
fn an_empty_version_reads_unknown() {
    let mut m = manifest("x");
    m.machines[0].version.clear();
    let got = issue(&m, "R-1").expect("redaction rules build");
    assert!(got.body.contains("node, linux 6.8, unknown: answered"));
}

#[test]
fn mentions_are_quoted_and_lone_at_signs_kept() {
    assert_eq!(
        quote_mentions("a @x/team, b@ c @ user@host.com"),
        "a `@x/team`, b@ c @ user@host.com"
    );
}

#[test]
fn remote_strings_stay_on_one_plain_line() {
    let mut m = manifest("x");
    m.machines[0].os_version = "6.8\n@x injected\n# heading".into();
    m.machines[0].version = "9".repeat(100);
    m.app = "desktop\n@y".into();
    let got = issue(&m, "R-1").expect("redaction rules build");
    assert!(!got.body.contains("@x"));
    assert!(!got.body.contains("@y"));
    assert!(!got.body.contains("\n# heading"));
    assert!(!got.body.contains(&"9".repeat(41)));
    assert!(got.body.contains("node, linux 6.8x injected heading, "));
}
