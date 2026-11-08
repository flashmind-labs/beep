use super::*;

#[test]
fn snippets_are_cut_around_the_match() {
    let text = format!("{} Kimpton {}", "a ".repeat(50), "b ".repeat(80));
    let at = find_ignoring_case(&text, "kimpton").unwrap();
    assert_eq!(&text[at.clone()], "Kimpton");
    let snippet = snippet(&text, at);
    assert!(snippet.starts_with("...a a"));
    assert!(snippet.ends_with("b b..."));
    assert!(snippet.contains("Kimpton"));
    assert_eq!(find_ignoring_case("ÉCOLE", "école"), Some(0..6));
    assert_eq!(match_snippet("no match", "boiler"), None);
}

#[test]
fn ascii_queries_match_like_any_other() {
    assert_eq!(
        find_ignoring_case("Deploy the SERVER", "server"),
        Some(11..17)
    );
    assert_eq!(find_ignoring_case("naïve Server", "server"), Some(7..13));
    assert_eq!(find_ignoring_case("short", "longer than it"), None);
    // The Kelvin sign lowercases to k, so it still matches.
    assert_eq!(
        find_ignoring_case("10 \u{212A}elvin", "kelvin"),
        Some(3..11)
    );
}
