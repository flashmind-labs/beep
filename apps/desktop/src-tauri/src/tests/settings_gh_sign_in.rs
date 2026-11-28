use super::*;

#[test]
fn the_code_and_page_are_read_from_gh() {
    let mut prompt = Prompt::default();
    assert_eq!(
        prompt.read("! First copy your one-time code: AB23-77BA"),
        None
    );
    assert_eq!(
        prompt
            .read("Open this URL to continue in your web browser: https://github.com/login/device"),
        Some(DeviceCode {
            code: "AB23-77BA".into(),
            url: "https://github.com/login/device".into(),
        })
    );
    // Once only, however gh repeats itself.
    assert_eq!(
        prompt.read("! First copy your one-time code: AB23-77BA"),
        None
    );
}

#[test]
fn the_page_may_come_first() {
    let mut prompt = Prompt::default();
    assert_eq!(
        prompt.read("Press Enter to open https://github.com/login/device in your browser..."),
        None
    );
    let device = prompt
        .read("! First copy your one-time code: 1F2E-3D4C")
        .expect("both known");
    assert_eq!(device.code, "1F2E-3D4C");
    assert_eq!(device.url, "https://github.com/login/device");
}

#[test]
fn only_a_code_is_taken_as_one() {
    assert_eq!(
        parse_code("! First copy your One-Time Code: AB23-77BA"),
        Some("AB23-77BA")
    );
    assert_eq!(parse_code("! First copy your one-time code:"), None);
    assert_eq!(parse_code("one-time code: <script>"), None);
    assert_eq!(parse_code("Authentication complete."), None);
}

#[test]
fn only_github_pages_open() {
    assert_eq!(
        parse_device_url("Open this URL: https://github.com/login/device.").map(|u| u.to_string()),
        Some("https://github.com/login/device".into())
    );
    assert_eq!(
        parse_device_url("Open https://example.com/login/device"),
        None
    );
    assert_eq!(
        parse_device_url("Open http://github.com/login/device"),
        None
    );
    assert_eq!(parse_device_url("no link here"), None);
}

#[test]
fn a_failure_says_what_gh_said() {
    assert_eq!(
        failure("X Failed to authenticate: expired_token\n"),
        "gh could not sign in to GitHub: Failed to authenticate: expired_token"
    );
    assert_eq!(failure("  "), "gh could not sign in to GitHub");
    assert!(failure(&"x".repeat(500)).len() < 260);
}

#[test]
fn one_sign_in_at_a_time() {
    let sign_in = GhSignIn::default();
    let _first = sign_in.start().expect("nothing under way");
    assert!(sign_in.start().is_err());
    sign_in.finish();
    assert!(sign_in.start().is_ok());
}

#[test]
fn a_cancelled_sign_in_holds_the_slot_until_gh_is_gone() {
    let sign_in = GhSignIn::default();
    let _first = sign_in.start().expect("nothing under way");
    sign_in.cancel();
    assert!(sign_in.start().is_err());
    sign_in.finish();
    assert!(sign_in.start().is_ok());
}

#[tokio::test]
async fn quitting_after_a_cancel_waits_for_gh_to_exit() {
    let sign_in = GhSignIn::default();
    let started = sign_in.start().expect("nothing under way");
    sign_in.cancel();
    let mut quit = Box::pin(sign_in.stop_on_quit());
    assert!(futures::poll!(quit.as_mut()).is_pending());
    drop(started.ended);
    quit.await;
}
