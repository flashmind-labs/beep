//! Replies arrive as Markdown; the bar shows them as HTML cleaned of
//! anything that could run. Math in `$x$`, `$$x$$`, `\(x\)` or `\[x\]`
//! becomes MathML, which WebKit draws natively.

use pulldown_cmark::{Event, Options, Parser, html};
use pulldown_latex::{ParserError, RenderConfig, Storage, config::DisplayMode, push_mathml};

// MathML Core elements, and the attributes pulldown-latex writes on them.
const MATH_TAGS: &[&str] = &[
    "math",
    "mi",
    "mn",
    "mo",
    "ms",
    "mtext",
    "mspace",
    "mrow",
    "mfrac",
    "msqrt",
    "mroot",
    "msub",
    "msup",
    "msubsup",
    "munder",
    "mover",
    "munderover",
    "mmultiscripts",
    "mprescripts",
    "none",
    "mtable",
    "mtr",
    "mtd",
    "mpadded",
    "mphantom",
    "mstyle",
    "merror",
    "semantics",
    "annotation",
];
const MATH_ATTRIBUTES: &[&str] = &[
    "class",
    "depth",
    "display",
    "displaystyle",
    "encoding",
    "height",
    "largeop",
    "linethickness",
    "lspace",
    "mathvariant",
    "maxsize",
    "minsize",
    "movablelimits",
    "rspace",
    "scriptlevel",
    "stretchy",
    "style",
    "symmetric",
    "width",
];
// The only properties pulldown-latex puts in a style attribute.
const STYLE_PROPERTIES: &[&str] = &["color", "border-color", "margin-left", "height"];

pub fn to_html(markdown: &str) -> String {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_GFM
        | Options::ENABLE_MATH;
    let source = dollar_delimiters(markdown);
    let events = Parser::new_ext(&source, options).map(|event| {
        let (latex, display, delim) = match &event {
            Event::InlineMath(latex) => (latex, DisplayMode::Inline, "$"),
            Event::DisplayMath(latex) => (latex, DisplayMode::Block, "$$"),
            _ => return event,
        };
        match mathml(latex, display) {
            Some(mathml) => Event::InlineHtml(mathml.into()),
            // Show what the model wrote rather than nothing.
            None => Event::Code(format!("{delim}{latex}{delim}").into()),
        }
    });
    let mut out = String::with_capacity(markdown.len() * 3 / 2);
    html::push_html(&mut out, events);
    // The renderer copies some LaTeX through unescaped (`\operatorname{<b>}`),
    // so its MathML goes through the cleaner like everything else.
    ammonia::Builder::default()
        .add_tags(MATH_TAGS)
        .add_generic_attributes(MATH_ATTRIBUTES)
        .attribute_filter(|_, attribute, value| match attribute {
            "style" if !safe_style(value) => None,
            _ => Some(value.into()),
        })
        .clean(&out)
        .to_string()
}

fn safe_style(style: &str) -> bool {
    style
        .split(';')
        .filter(|declaration| !declaration.trim().is_empty())
        .all(|declaration| {
            let Some((property, value)) = declaration.split_once(':') else {
                return false;
            };
            STYLE_PROPERTIES.contains(&property.trim())
                && value
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || " #.%()-".contains(c))
                && !value.contains("url")
        })
}

/// The formula as MathML, or None when it does not parse.
fn mathml(latex: &str, display_mode: DisplayMode) -> Option<String> {
    let storage = Storage::new();
    let events = pulldown_latex::Parser::new(latex, &storage)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    let config = RenderConfig {
        display_mode,
        ..RenderConfig::default()
    };
    let mut out = String::new();
    push_mathml(
        &mut out,
        events.into_iter().map(Ok::<_, ParserError>),
        config,
    )
    .ok()?;
    Some(out)
}

/// Rewrites `\(x\)` as `$x$` and `\[x\]` as `$$x$$`, which is all the
/// Markdown parser knows, leaving code spans and fenced blocks alone.
fn dollar_delimiters(markdown: &str) -> String {
    if !markdown.contains("\\(") && !markdown.contains("\\[") {
        return markdown.to_owned();
    }
    let mut out = String::with_capacity(markdown.len());
    let mut fenced = false;
    // Display math may span lines, so the open delimiter carries over.
    let mut open: Option<&str> = None;
    for line in markdown.split_inclusive('\n') {
        if open.is_none() && line.trim_start().starts_with("```") {
            fenced = !fenced;
        }
        if fenced {
            out.push_str(line);
            continue;
        }
        let mut rest = line;
        while !rest.is_empty() {
            let next = match open {
                Some(close) => rest.find(close).map(|i| (i, close)),
                None => ["`", "\\(", "\\["]
                    .iter()
                    .filter_map(|token| rest.find(token).map(|i| (i, *token)))
                    .min_by_key(|(i, _)| *i),
            };
            let Some((i, token)) = next else {
                out.push_str(rest);
                break;
            };
            out.push_str(&rest[..i]);
            rest = &rest[i + token.len()..];
            match token {
                "`" => {
                    let end = rest.find('`').map_or(rest.len(), |end| end + 1);
                    out.push('`');
                    out.push_str(&rest[..end]);
                    rest = &rest[end..];
                }
                "\\(" => {
                    out.push('$');
                    open = Some("\\)");
                }
                "\\[" => {
                    out.push_str("$$");
                    open = Some("\\]");
                }
                "\\)" => {
                    out.push('$');
                    open = None;
                }
                _ => {
                    out.push_str("$$");
                    open = None;
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_markdown_and_drops_scripts() {
        let html = to_html("**hi** <script>alert(1)</script> [x](javascript:alert(1))");
        assert!(html.contains("<strong>hi</strong>"));
        assert!(!html.contains("script"));
        assert!(!html.contains("javascript:"));
    }

    #[test]
    fn renders_math_in_every_delimiter() {
        for source in [
            "so $A = [s]B$ holds",
            r"so \(A = [s]B\) holds",
            "$$s = \\text{clamp}(x), \\qquad A = [s]B$$",
            "\\[\ns \\ne x\n\\]",
        ] {
            let html = to_html(source);
            assert!(html.contains("<math"), "{source} -> {html}");
        }
        assert!(to_html("$$x$$").contains(r#"display="block""#));
    }

    #[test]
    fn leaves_prices_and_code_alone() {
        assert!(!to_html("it costs $5 and $10").contains("<math"));
        assert!(!to_html(r"`\(x\)` and `$y$`").contains("<math"));
        assert!(!to_html("```\n\\[x\\]\n```").contains("<math"));
    }

    #[test]
    fn math_is_cleaned_too() {
        let html = to_html(
            r#"$\operatorname{<img src=x onerror=alert(1)>}$ <math><mi onclick="x" style="position:fixed">a</mi></math>"#,
        );
        assert!(html.contains("<math"), "{html}");
        assert!(!html.contains("onerror"), "{html}");
        assert!(!html.contains("onclick"), "{html}");
        assert!(!html.contains("position"), "{html}");
        let colored = to_html(r"$\color{red}{a}$");
        assert!(colored.contains("style=\"color: rgb("), "{colored}");
    }

    #[test]
    fn bad_latex_shows_as_code() {
        let html = to_html(r"see $\frac{1$ here");
        assert!(!html.contains("<math"), "{html}");
    }
}
