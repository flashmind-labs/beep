use super::*;

#[test]
fn reads_tokens_by_their_css_name() {
    assert_eq!(
        "--radius".parse::<StyleToken>().unwrap(),
        StyleToken::Radius
    );
    assert_eq!(StyleToken::GlassSolid.to_string(), "--glass-solid");
    assert!("--ease".parse::<StyleToken>().is_err());
    assert!("radius".parse::<StyleToken>().is_err());
}

#[test]
fn refuses_values_that_load_or_escape() {
    for value in [
        "url(https://example.com/a.png)",
        "URL(x)",
        "image-set(\"a.png\" 1x)",
        "red; color: blue",
        "a } b {",
        "\\75rl(x)",
        "@import",
        "",
        "   ",
        &"a".repeat(MAX_STYLE_VALUE + 1),
    ] {
        assert!(
            StyleValue::try_from(value.to_owned()).is_err(),
            "{value:?} should be refused"
        );
    }
    for value in [
        "#101010",
        "0",
        "blur(12px) saturate(1.2)",
        "\"IBM Plex Mono\", monospace",
        "none",
    ] {
        assert!(
            StyleValue::try_from(value.to_owned()).is_ok(),
            "{value:?} should be accepted"
        );
    }
}
