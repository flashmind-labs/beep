use super::*;

fn style(pairs: &[(&str, &str)]) -> Style {
    pairs
        .iter()
        .map(|(token, value)| {
            let token = token.parse::<StyleToken>().unwrap();
            let value = StyleValue::try_from((*value).to_owned()).unwrap();
            (token, value)
        })
        .collect()
}

fn id(text: &str) -> PluginId {
    text.parse().unwrap()
}

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

#[test]
fn the_plugin_turned_on_last_wins_and_the_clash_is_listed() {
    let (sharp, round, calm) = (id("sharp"), id("round"), id("calm"));
    let sharp_style = style(&[("--radius", "0"), ("--accent", "#f00")]);
    let round_style = style(&[("--radius", "12px")]);
    let calm_style = style(&[("--paintings", "none")]);

    let resolved = resolve(vec![
        Layer {
            id: &round,
            order: 3,
            style: &round_style,
        },
        Layer {
            id: &sharp,
            order: 1,
            style: &sharp_style,
        },
        Layer {
            id: &calm,
            order: 2,
            style: &calm_style,
        },
    ]);

    assert_eq!(resolved.tokens[&StyleToken::Radius].as_ref(), "12px");
    assert_eq!(resolved.tokens[&StyleToken::Accent].as_ref(), "#f00");
    assert_eq!(resolved.tokens[&StyleToken::Paintings].as_ref(), "none");
    assert_eq!(
        resolved.conflicts,
        [StyleConflict {
            token: StyleToken::Radius,
            plugins: vec![sharp, round]
        }]
    );
}

#[test]
fn plugins_turned_on_together_follow_their_ids() {
    let (a, b) = (id("aa"), id("bb"));
    let a_style = style(&[("--radius", "1px")]);
    let b_style = style(&[("--radius", "2px")]);

    let resolved = resolve(vec![
        Layer {
            id: &b,
            order: 0,
            style: &b_style,
        },
        Layer {
            id: &a,
            order: 0,
            style: &a_style,
        },
    ]);

    assert_eq!(resolved.tokens[&StyleToken::Radius].as_ref(), "2px");
}
