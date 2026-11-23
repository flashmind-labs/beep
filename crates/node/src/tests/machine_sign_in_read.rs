use super::*;

const CLAUDE_URL: &str = "https://claude.com/cai/oauth/authorize?code=true&client_id=00000000-0000-0000-0000-000000000000&response_type=code&redirect_uri=https%3A%2F%2Fplatform.claude.com%2Foauth%2Fcode%2Fcallback&scope=user%3Ainference&code_challenge=abc&code_challenge_method=S256&state=xyz";

/// What `claude auth login` 2.1 prints on pipes.
fn claude_login() -> String {
    format!(
        "Opening browser to sign in\u{2026}\nIf the browser didn't open, visit: {CLAUDE_URL}\nPaste code here if prompted > "
    )
}

#[test]
fn claude_gives_its_link_and_asks_for_a_code() {
    assert_eq!(
        prompt(Tool::Claude, &claude_login()),
        Some(State::Waiting {
            url: CLAUDE_URL.into(),
            code: None,
            asks_code: true,
        })
    );
    // The link's line has not ended yet.
    let half = format!("If the browser didn't open, visit: {}", &CLAUDE_URL[..60]);
    assert_eq!(prompt(Tool::Claude, &half), None);
}

#[test]
fn a_hyperlink_on_a_terminal_reads_once() {
    let drawn = format!(
        "visit: \u{1b}]8;;{CLAUDE_URL}\u{7}{CLAUDE_URL}\u{1b}]8;;\u{7}\r\nPaste code here if prompted > "
    );
    let Some(State::Waiting { url, .. }) = prompt(Tool::Claude, &drawn) else {
        panic!("no prompt in {drawn:?}");
    };
    assert_eq!(url, CLAUDE_URL);
}

/// What codex-cli 0.166 prints, colours and all.
const DEVICE_AUTH: &str = "\n\u{1b}[1mWelcome to Codex\u{1b}[0m [v\u{1b}[90m0.166.0\u{1b}[0m]\n\
    Follow these steps to sign in with ChatGPT using device code authorization:\n\n\
    1. Open this link in your browser and sign in to your account\n   \
    \u{1b}[94mhttps://auth.openai.com/codex/device\u{1b}[0m\n\n\
    2. Enter this one-time code \u{1b}[90m(expires in 15 minutes)\u{1b}[0m\n   \
    \u{1b}[94mUZIW-O7IAI\u{1b}[0m\n\n";

#[test]
fn codex_gives_its_page_and_code() {
    assert_eq!(
        prompt(Tool::Codex, DEVICE_AUTH),
        Some(State::Waiting {
            url: "https://auth.openai.com/codex/device".into(),
            code: Some("UZIW-O7IAI".into()),
            asks_code: false,
        })
    );
    let half: String = DEVICE_AUTH
        .lines()
        .take(5)
        .map(|l| format!("{l}\n"))
        .collect();
    assert_eq!(prompt(Tool::Codex, &half), None);
}

#[test]
fn opencode_gives_its_page_and_code() {
    // What opencode 1.19 prints for ChatGPT's headless method, on pipes.
    let printed = "\u{1b}[0m\n\u{250c}  Add credential\n\u{2502}\n\
        \u{25cf}  Go to: https://auth.openai.com/codex/device\n\u{2502}\n\
        \u{25cf}  Enter code: 3590-11S3E\n\u{1b}[?25l\u{2502}\n\
        \u{25d2}  Waiting for authorization\u{1b}[999D\u{1b}[J";
    assert_eq!(
        prompt(Tool::Opencode, printed),
        Some(State::Waiting {
            url: "https://auth.openai.com/codex/device".into(),
            code: Some("3590-11S3E".into()),
            asks_code: false,
        })
    );
}

#[test]
fn antigravity_gives_its_link_and_asks_for_a_code() {
    // What agy 1.5 prints in print mode with JSON events, on pipes.
    let url = "https://accounts.google.com/o/oauth2/auth?access_type=offline&client_id=0000-abc.apps.googleusercontent.com&code_challenge=abc&redirect_uri=https%3A%2F%2Fantigravity.google%2Foauth-callback&response_type=code&state=xyz";
    let printed = format!(
        "Authentication required. Please visit the URL to log in:\n  {url}\n\n\
         Waiting for authentication (timeout 60s)...\n\
         Or, paste the authorization code here and press Enter:\n"
    );
    assert_eq!(
        prompt(Tool::Agy, &printed),
        Some(State::Waiting {
            url: url.into(),
            code: None,
            asks_code: true,
        })
    );
    let ended = format!(
        "{printed}\nError: authentication interrupted.\nerror: authentication failed or timed out\n\
         {{\"event\":\"result\",\"result\":{{\"status\":\"ERROR\"}}}}\n"
    );
    assert_eq!(
        failure("agy", &ended),
        "agy could not sign in: error: authentication failed or timed out"
    );
}

#[test]
fn only_the_agents_own_links_are_passed_on() {
    let openai = hosts(Tool::Codex);
    assert!(link("https://auth.openai.com/codex/device", openai).is_some());
    assert!(link("(https://chatgpt.com/device).", openai).is_some());
    assert_eq!(link("http://auth.openai.com/codex/device", openai), None);
    assert_eq!(link("https://openai.com.example.net/device", openai), None);
    assert_eq!(
        link("https://example.com/claude.com", hosts(Tool::Claude)),
        None
    );
}

#[test]
fn a_failure_says_the_last_line() {
    assert_eq!(
        failure(
            "claude",
            "Opening browser\n\u{1b}[31mOAuth error: invalid code\u{1b}[39m\n\n"
        ),
        "claude could not sign in: OAuth error: invalid code"
    );
    assert_eq!(failure("codex", ""), "codex could not sign in.");
}
