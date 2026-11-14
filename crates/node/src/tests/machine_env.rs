use super::*;

#[test]
fn reads_opencode_auth_list() {
    let one = "\u{1b}[0m\n\u{250c}  Credentials \u{1b}[90m~/.local/share/opencode/auth.json\n\
        \u{2502}\n\u{25cf}  Anthropic \u{1b}[90moauth\n\u{2502}\n\u{2514}  1 credentials\n\n\
        \u{250c}  Environment\n\u{2502}\n\u{2514}  2 environment variables\n";
    assert_eq!(opencode_logged_in(one), Some(true));
    let none = "\u{250c}  Credentials\n\u{2502}\n\u{2514}  0 credentials\n";
    assert_eq!(opencode_logged_in(none), Some(false));
    assert_eq!(
        opencode_logged_in("Error: something went wrong"),
        Some(false)
    );
}

#[test]
fn avd_list_skips_log_lines() {
    let text = "INFO    | Storing crashdata in: /tmp/x\nPixel_7\nPixel_9_Pro\n";
    assert_eq!(avd_names(text), ["Pixel_7", "Pixel_9_Pro"]);
}

#[test]
fn tool_versions_are_bare() {
    let cases = [
        ("claude", "2.1.312 (Claude Code)", "2.1.312"),
        ("codex", "codex-cli 0.162.1", "0.162.1"),
        ("opencode", "opencode v2.2.5", "2.2.5"),
        ("opencode", "1.3.13", "1.3.13"),
        ("go", "go version go1.22.0 linux/amd64", "1.22.0"),
        ("docker", "Docker version 27.3.1, build ce12230", "27.3.1"),
        ("kubectl", "Client Version: v1.31.2", "1.31.2"),
        ("node", "v22.11.0", "22.11.0"),
        ("xcodebuild", "Xcode 16.1", "16.1"),
    ];

    for (name, line, want) in cases {
        assert_eq!(tool_version(name, line), want, "{name}: {line}");
    }

    assert_eq!(tool_version("tool", "no version here"), "no version here");
}

#[cfg(unix)]
#[tokio::test]
async fn claude_login_respects_the_status_command() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let program = dir.path().join("claude");
    let cases = [
        (r#"{"loggedIn":false,"authMethod":"none"}"#, 0, false, None),
        (r#"{"loggedIn":false,"authMethod":"none"}"#, 1, false, None),
        (
            r#"{"loggedIn":true,"email":"you@example.com"}"#,
            0,
            true,
            Some("you@example.com"),
        ),
    ];

    for (status, exit, signed_in, account) in cases {
        tokio::fs::write(
            &program,
            format!("#!/bin/sh\nprintf '%s\\n' '{status}'\nexit {exit}\n"),
        )
        .await
        .unwrap();
        tokio::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700))
            .await
            .unwrap();

        let read = login(
            "claude",
            &program,
            &["auth", "status"],
            claude_logged_in,
            claude_account,
        )
        .await;

        assert_eq!(read.signed_in, signed_in);
        assert_eq!(read.account.as_deref(), account);
    }
}
