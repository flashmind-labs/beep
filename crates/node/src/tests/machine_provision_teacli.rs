use super::*;
use beep_proto::SecretText;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

fn login(url: &str, token: &str) -> ForgejoLogin {
    ForgejoLogin {
        url: url.into(),
        token: SecretText::new(token),
    }
}

#[test]
fn beeps_servers_join_the_users_own() {
    let user = "[global]\ndefault_server = work\n\n[work]\nurl = https://git.example.com/\ntoken = old\n\n\
                [home]\n# mine\nurl = http://code.example.org\ntoken = mine\n";
    let text = render(
        &[
            login("https://git.example.com", "fresh"),
            login("https://forge.example.net:3000", "other"),
        ],
        Some(user),
    );
    // The user's section for the same URL keeps its name, with Beep's token.
    assert!(
        text.contains("[work]\nurl = https://git.example.com\ntoken = fresh\n"),
        "{text}"
    );
    assert!(!text.contains("token = old"), "{text}");
    assert!(
        text.contains(
            "[forge.example.net-3000]\nurl = https://forge.example.net:3000\ntoken = other\n"
        ),
        "{text}"
    );
    assert!(
        text.contains("[home]\n# mine\nurl = http://code.example.org\ntoken = mine\n"),
        "{text}"
    );
    assert!(text.contains("[global]\ndefault_server = work\n"), "{text}");
}

#[test]
fn the_config_is_private_and_goes_with_the_last_login() {
    let dir = std::env::temp_dir().join(format!("beep-node-teacli-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let teacli = Path::new("/opt/it's/teacli");
    assert!(
        configure(
            &dir,
            &[login("https://git.example.com", "t")],
            None,
            Some(teacli)
        )
        .unwrap()
    );
    let config = config_path(&dir);
    #[cfg(unix)]
    assert_eq!(fs::metadata(&config).unwrap().mode() & 0o777, 0o600);
    let shim = fs::read_to_string(shims_dir(&dir).join(SHIM)).unwrap();
    if cfg!(windows) {
        assert!(shim.contains("\"/opt/it's/teacli\" --config \""), "{shim}");
    } else {
        assert!(
            shim.contains("teacli='/opt/it'\\''s/teacli'") && shim.contains("--config '"),
            "{shim}"
        );
    }

    assert!(!configure(&dir, &[], None, Some(teacli)).unwrap());
    assert!(!config.exists());
    assert!(!shims_dir(&dir).join(SHIM).exists());
    fs::remove_dir_all(&dir).unwrap();
}
