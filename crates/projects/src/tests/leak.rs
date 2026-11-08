use super::*;

fn public() -> PublicText {
    PublicText::new(
        &["build-box".to_owned(), "nas-box".to_owned()],
        &["/home/sam".to_owned()],
    )
}

#[test]
fn extra_paths_are_private_without_their_names() {
    let public = public().with_paths(&["/srv/widget".to_owned()]);
    assert_eq!(public.hits("built in /srv/widget/out"), ["/srv/widget"]);
    // The folder's name alone is the repository's, which is public.
    assert!(public.hits("the widget repo").is_empty());
}

#[test]
fn public_text_hits() {
    let public = public();
    assert_eq!(public.hits("Built on build-box"), ["build-box"]);
    assert_eq!(public.hits("nas-box"), ["nas-box"]);
    assert!(
        public
            .hits("/home/sam/src")
            .contains(&"/home/sam".to_owned())
    );
    assert_eq!(public.hits("thanks sam."), ["sam"]);
    assert!(public.hits("a sample").is_empty());
    assert_eq!(public.hits("at 10.0.0.12 now"), ["10.0.0.12"]);
    assert_eq!(public.hits("on [fd00::1]:8080"), ["[fd00::1]:8080"]);
    assert_eq!(public.hits("fe80::1 and fe80::1"), ["fe80::1"]);
    assert_eq!(public.hits("db:5432"), ["db:5432"]);
    assert_eq!(public.hits("localhost:8080"), ["localhost:8080"]);
    assert!(public.hits("Fix the build box layout").is_empty());
    assert!(public.hits("version 1.2.3").is_empty());
    assert!(public.hits("localhost is fine").is_empty());
    assert!(public.hits("meet at 12:30:45 or 12:30").is_empty());
    assert!(public.hits("std::fmt and Vec::new").is_empty());
    for text in [
        "leak.rs:120",
        "Cargo.toml:14",
        "at 2026-11-01T12:30",
        "x:70000",
    ] {
        assert!(public.hits(text).is_empty(), "{text}");
    }
}

#[test]
fn ipv6_edges_are_kept() {
    let public = public();
    for literal in ["::1", "fe80::", "::ffff:10.0.0.1"] {
        assert_eq!(public.hits(&format!("see {literal}.")), [literal]);
    }
}

#[test]
fn home_paths_end_at_the_name() {
    let public = public();
    assert!(public.hits("/home/samuel/src").is_empty());
    assert!(
        public
            .hits("/HOME/Sam/src")
            .contains(&"/home/sam".to_owned())
    );
}

#[test]
fn generic_and_short_names_are_not_words() {
    let public = PublicText::new(
        &["pi".to_owned()],
        &["/root".to_owned(), "/home/user".to_owned()],
    );
    assert!(public.hits("the root user runs pi").is_empty());
    assert_eq!(public.hits("/home/user/src"), ["/home/user"]);
    assert_eq!(public.hits("cd /root"), ["/root"]);
}
