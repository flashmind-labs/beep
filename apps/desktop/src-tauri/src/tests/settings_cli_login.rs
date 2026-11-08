use super::*;

#[test]
fn a_printed_token_is_read_and_nothing_else_is() {
    let token = parse_token("gho_abc123\n").expect("one token");
    assert_eq!(token.expose(), "gho_abc123");
    assert!(parse_token("  \n").is_none());
    assert!(parse_token("no oauth token found for github.com").is_none());
}

#[test]
fn a_login_is_what_github_allows() {
    assert_eq!(parse_login("you-work\n").as_deref(), Some("you-work"));
    assert_eq!(parse_login(""), None);
    assert_eq!(parse_login("{\"message\":\"Bad credentials\"}"), None);
}

const STATUS_TWO: &str = "github.com
  \u{2713} Logged in to github.com account octo-one (keyring)
  - Active account: false
  - Git operations protocol: https
  - Token: gho_************************************
  - Token scopes: 'gist', 'read:org', 'repo', 'workflow'

  \u{2713} Logged in to github.com account octo-two (keyring)
  - Active account: true
  - Git operations protocol: https
  - Token: gho_************************************
  - Token scopes: 'gist', 'read:org', 'repo'

  X Failed to log in to github.com account octo-three (keyring)
  - Active account: false
  - The token in keyring is invalid.
";

#[test]
fn gh_status_lists_each_account_and_the_active_one() {
    assert_eq!(
        parse_gh_status(STATUS_TWO),
        [
            GhAccount {
                login: "octo-one".into(),
                active: false
            },
            GhAccount {
                login: "octo-two".into(),
                active: true
            },
        ]
    );
}

#[test]
fn gh_status_with_one_account() {
    let one = "github.com\n  \u{2713} Logged in to github.com account octo-one (keyring)\n  \
                   - Active account: true\n  - Git operations protocol: https\n";
    assert_eq!(
        parse_gh_status(one),
        [GhAccount {
            login: "octo-one".into(),
            active: true
        }]
    );
    // A gh from before several accounts.
    let old = "github.com\n  \u{2713} Logged in to github.com as octo-one \
                   (/srv/home/.config/gh/hosts.yml)\n  \u{2713} Git operations for github.com \
                   configured to use https protocol.\n  \u{2713} Token: gho_*****\n";
    assert_eq!(
        parse_gh_status(old),
        [GhAccount {
            login: "octo-one".into(),
            active: true
        }]
    );
    assert!(
        parse_gh_status(
            "You are not logged into any GitHub hosts. To log in, run: gh auth login\n"
        )
        .is_empty()
    );
}

#[cfg(windows)]
#[test]
fn windows_tools_use_native_paths_and_the_user_profile() {
    let tools = PathBuf::from(r"C:\Tools");
    let home = PathBuf::from(r"C:\Profile");
    let path = env::join_paths([tools.as_path(), Path::new("relative")]).expect("test PATH");
    assert_eq!(
        tool_dirs(&path, Some(&home)),
        vec![tools, home.join(".local/bin")]
    );
}

#[cfg(unix)]
#[test]
fn tools_are_looked_for_past_a_bare_path() {
    let dirs = tool_dirs(
        OsStr::new("/usr/bin:/bin:relative"),
        Some(Path::new("/home/me")),
    );
    let dirs: Vec<_> = dirs
        .iter()
        .map(|d| d.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        dirs,
        [
            "/usr/bin",
            "/bin",
            "/opt/homebrew/bin",
            "/usr/local/bin",
            "/home/me/.local/bin"
        ]
    );
    let once = tool_dirs(OsStr::new("/opt/homebrew/bin"), None);
    assert_eq!(once.len(), 2);
}

#[test]
fn teacli_servers_with_a_token_are_read() {
    let ini = "# managed elsewhere\n[global]\ndefault_server = work\n\n\
                   [work]\nurl   = https://git.example.com\ntoken = abc123\n\n\
                   [half]\nurl = https://code.example.org\n\n\
                   [bad]\nurl = not a url\ntoken = x\n\n\
                   [last]\n; comment\ntoken=def456\nurl=http://git.example.net:3000\n";
    let logins = parse_tea(ini);
    let found: Vec<_> = logins
        .iter()
        .map(|l| (l.url.as_str(), l.token.expose()))
        .collect();
    assert_eq!(
        found,
        [
            ("https://git.example.com", "abc123"),
            ("http://git.example.net:3000", "def456")
        ]
    );
}

#[test]
fn servers_match_by_scheme_host_and_port() {
    assert!(same_server(
        "https://git.example.com",
        "https://git.example.com/"
    ));
    assert!(same_server(
        "https://Git.Example.com:443",
        "https://git.example.com"
    ));
    assert!(!same_server(
        "http://git.example.com",
        "https://git.example.com"
    ));
    assert!(!same_server(
        "https://git.example.com:3000",
        "https://git.example.com"
    ));
    assert!(!same_server("git.example.com", "git.example.com"));
}

#[test]
fn the_bar_never_gets_the_token() {
    let login = Login::SignedIn {
        token: SecretText::new("gho_secret"),
        account: Some("you".into()),
    };
    let json = serde_json::to_string(&LoginView::from(&login)).expect("serializes");
    assert_eq!(json, r#"{"state":"signed_in","account":"you"}"#);
    assert!(!json.contains("gho_secret"));
}
