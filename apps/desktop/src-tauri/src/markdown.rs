//! Replies arrive as Markdown; the bar shows them as HTML cleaned of
//! anything that could run. Math in `$x$`, `$$x$$`, `\(x\)` or `\[x\]`
//! becomes MathML, which WebKit draws natively. ```product blocks become
//! product cards.

use std::ops::Range;

use pulldown_cmark::{CodeBlockKind, CowStr, Event, Options, Parser, Tag, TagEnd, html};
use pulldown_latex::{ParserError, RenderConfig, Storage, config::DisplayMode, push_mathml};

use crate::products::{self, Product};

// Stands in for a row of product cards until the HTML is cleaned. A
// private-use character, removed from the reply first so it cannot fake one.
const ROW_MARK: char = '\u{E000}';

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
    let source = dollar_delimiters(&markdown.replace(ROW_MARK, ""));
    let mut rows = Vec::new();
    let parsed = Parser::new_ext(&source, options).into_offset_iter();
    let events = product_rows(&source, parsed, &mut rows)
        .into_iter()
        .map(|event| {
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
    let mut clean = ammonia::Builder::default()
        .add_tags(MATH_TAGS)
        .add_generic_attributes(MATH_ATTRIBUTES)
        .attribute_filter(|_, attribute, value| match attribute {
            "style" if !safe_style(value) => None,
            _ => Some(value.into()),
        })
        .clean(&out)
        .to_string();
    // The cards are built from escaped fields, so they go in after cleaning.
    for (i, cards) in rows.iter().enumerate() {
        clean = clean.replacen(&row_mark(i), &products::row(cards), 1);
    }
    clean
}

fn row_mark(i: usize) -> String {
    format!("{ROW_MARK}{i}{ROW_MARK}")
}

/// Replaces ```product blocks with row marks, collecting each row's cards.
/// A block that is not a product stays a code block, unless it is still
/// streaming in, when it shows as a card waiting for its contents.
fn product_rows<'a>(
    source: &str,
    parsed: impl Iterator<Item = (Event<'a>, Range<usize>)>,
    rows: &mut Vec<Vec<String>>,
) -> Vec<Event<'a>> {
    let mut out = Vec::new();
    // Set while the last event out is a row mark, so the next card joins it.
    let mut open_row = false;
    let mut block: Option<(Vec<Event<'a>>, Range<usize>)> = None;
    for (event, range) in parsed {
        let Some((events, _)) = block.as_mut() else {
            match &event {
                Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(lang)))
                    if lang.trim() == "product" =>
                {
                    block = Some((vec![event], range));
                }
                _ => {
                    out.push(event);
                    open_row = false;
                }
            }
            continue;
        };
        let end = matches!(event, Event::End(TagEnd::CodeBlock));
        events.push(event);
        if !end {
            continue;
        }
        let Some((events, range)) = block.take() else {
            continue;
        };
        let json: String = events
            .iter()
            .filter_map(|e| match e {
                Event::Text(text) => Some(text.as_ref()),
                _ => None,
            })
            .collect();
        let card = match Product::parse(&json) {
            Some(product) => product.card(),
            None if !closed(&source[range]) => products::pending_card().to_owned(),
            None => {
                out.extend(events);
                open_row = false;
                continue;
            }
        };
        if !open_row {
            out.push(Event::Text(CowStr::from(row_mark(rows.len()))));
            rows.push(Vec::new());
            open_row = true;
        }
        if let Some(row) = rows.last_mut() {
            row.push(card);
        }
    }
    out
}

/// Whether a fenced block's source ends with its closing fence.
fn closed(block: &str) -> bool {
    let mut lines = block.trim_end().lines();
    let Some(open) = lines.next().and_then(Fence::open) else {
        return false;
    };
    lines.last().is_some_and(|last| open.closed_by(last))
}

/// The line that opens a fenced code block.
struct Fence<'a> {
    /// The fence's character, a backtick or a tilde.
    mark: u8,
    /// How many of it, at least three.
    len: usize,
    /// What follows the fence on its line.
    info: &'a str,
}

impl<'a> Fence<'a> {
    /// The fence `line` opens with, past any indent or blockquote markers.
    fn open(line: &'a str) -> Option<Self> {
        let line = line.trim_start_matches([' ', '\t', '>']);
        let mark = *line.as_bytes().first()?;
        if mark != b'`' && mark != b'~' {
            return None;
        }
        let len = line.bytes().take_while(|&b| b == mark).count();
        let info = &line[len..];
        // A backtick fence's info string may not hold a backtick.
        if len < 3 || (mark == b'`' && info.contains('`')) {
            return None;
        }
        Some(Self { mark, len, info })
    }

    /// Whether `line` closes this fence: as many of the same character or
    /// more, and nothing after them.
    fn closed_by(&self, line: &str) -> bool {
        Fence::open(line).is_some_and(|close| {
            close.mark == self.mark && close.len >= self.len && close.info.trim().is_empty()
        })
    }
}

/// Where the code span closed by a run of exactly `ticks` backticks ends in
/// `text`, or `None` when no such run follows.
fn code_span_end(text: &str, ticks: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'`' {
            i += 1;
            continue;
        }
        let run = bytes[i..].iter().take_while(|&&b| b == b'`').count();
        i += run;
        if run == ticks {
            return Some(i);
        }
    }
    None
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
    // pulldown-latex writes `\ ` and `~` as an escaped `&nbsp;`, which would
    // show as text.
    Some(out.replace("&amp;nbsp;", "\u{a0}"))
}

/// Rewrites `\(x\)` as `$x$` and `\[x\]` as `$$x$$`, which is all the
/// Markdown parser knows, and escapes dollars that are prices, leaving code
/// spans and fenced blocks alone.
fn dollar_delimiters(markdown: &str) -> String {
    if !markdown.contains(['$', '\\']) {
        return markdown.to_owned();
    }
    let mut out = String::with_capacity(markdown.len());
    let mut fenced: Option<Fence> = None;
    // Display math may span lines, so the open delimiter carries over.
    let mut open: Option<&str> = None;
    for line in markdown.split_inclusive('\n') {
        if let Some(fence) = &fenced {
            if fence.closed_by(line) {
                fenced = None;
            }
            out.push_str(line);
            continue;
        }
        if open.is_none()
            && let Some(fence) = Fence::open(line)
        {
            fenced = Some(fence);
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
                push_text(&mut out, rest, open.is_some());
                break;
            };
            push_text(&mut out, &rest[..i], open.is_some());
            rest = &rest[i + token.len()..];
            match token {
                "`" => {
                    // A span opened by N backticks closes at the next run of
                    // exactly N; without one the backticks are plain text.
                    let ticks = 1 + rest.bytes().take_while(|&b| b == b'`').count();
                    let end = code_span_end(&rest[ticks - 1..], ticks)
                        .map_or(ticks - 1, |end| end + ticks - 1);
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

fn push_text(out: &mut String, text: &str, math: bool) {
    if math {
        out.push_str(text);
    } else {
        escape_prices(out, text);
    }
}

/// Pairs single dollars the way Pandoc does and escapes the ones left over,
/// so "$4,957 of which $85" stays text. An opening `$` has no space after
/// it; a closing one has no space before it and no digit after it.
fn escape_prices(out: &mut String, text: &str) {
    let bytes = text.as_bytes();
    let at = |i: usize| bytes.get(i).copied();
    // Lone dollars only: `\$` is already text and `$$` is display math.
    let mut dollars = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 1,
            b'$' if at(i + 1) == Some(b'$') => i += 1,
            b'$' => dollars.push(i),
            _ => {}
        }
        i += 1;
    }
    let opens = |i: usize| at(i + 1).is_some_and(|c| !c.is_ascii_whitespace());
    let closes = |i: usize| {
        i > 0
            && !bytes[i - 1].is_ascii_whitespace()
            && !at(i + 1).is_some_and(|c| c.is_ascii_digit())
    };
    let mut prices = Vec::new();
    let mut k = 0;
    while k < dollars.len() {
        let open = dollars[k];
        match dollars[k + 1..].iter().position(|&d| closes(d)) {
            Some(n) if opens(open) => k += n + 2,
            _ => {
                prices.push(open);
                k += 1;
            }
        }
    }
    let mut last = 0;
    for p in prices {
        out.push_str(&text[last..p]);
        out.push('\\');
        last = p;
    }
    out.push_str(&text[last..]);
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
        let html = to_html("**fees: ~$4,957**, of which&nbsp;$85 is pending");
        assert!(!html.contains("<math"), "{html}");
        assert!(html.contains("<strong>fees: ~$4,957</strong>"), "{html}");
        assert!(to_html("$x$ costs $5").contains("<math"));
        assert!(to_html(r"$a$, $b$ and \$c").matches("<math").count() == 2);
        assert!(!to_html(r"`\(x\)` and `$y$`").contains("<math"));
        assert!(!to_html("```\n\\[x\\]\n```").contains("<math"));
    }

    #[test]
    fn leaves_every_kind_of_code_alone() {
        for code in [
            "~~~\n\\[x\\] costs $5\n~~~\n",
            "````\n```\n\\[x\\] costs $5\n```\n````\n",
            "> ```\n> \\[x\\] costs $5\n> ```\n",
            "``a ` \\(x\\) costs $5``\n",
        ] {
            assert_eq!(dollar_delimiters(code), code);
        }
        // Text after a closed fence is still read.
        assert!(to_html("~~~\ncode\n~~~\n\\(x\\)").contains("<math"));
        assert!(to_html("````\n```\n````\n\\(x\\)").contains("<math"));
        // A lone backtick is text, so the math after it still renders.
        assert!(to_html(r"a ` then \(x\)").contains("<math"));
    }

    #[test]
    fn latex_spaces_are_spaces() {
        let html = to_html(r"$m > 0,\ n = 0$ and $a~b$");
        assert!(!html.contains("&amp;"), "{html}");
        assert_eq!(html.matches("<mtext>&nbsp;</mtext>").count(), 2, "{html}");
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
    fn product_blocks_in_a_row_share_one_row() {
        let a = r#"{"title":"Kettle","url":"https://shop.example/k","price":"€40"}"#;
        let b = r#"{"title":"Toaster","url":"https://shop.example/t"}"#;
        let html = to_html(&format!(
            "Two picks:\n\n```product\n{a}\n```\n\n```product\n{b}\n```\n\nThe kettle is better."
        ));
        assert_eq!(html.matches("class=\"dprods\"").count(), 1, "{html}");
        assert_eq!(html.matches("class=\"dprod\"").count(), 2, "{html}");
        assert!(html.contains("<p>The kettle is better.</p>"), "{html}");
        assert!(!html.contains(ROW_MARK), "{html}");
        assert!(!html.contains("<pre>"), "{html}");

        let apart = to_html(&format!("```product\n{a}\n```\nand\n```product\n{b}\n```"));
        assert_eq!(apart.matches("class=\"dprods\"").count(), 2, "{apart}");
    }

    #[test]
    fn bad_product_shows_as_code_unless_still_streaming() {
        let html = to_html("```product\n{\"title\": oops}\n```");
        assert!(html.contains("<pre>"), "{html}");
        assert!(!html.contains("dprod"), "{html}");

        let streaming = to_html("Picks:\n\n```product\n{\"title\":\"Ket");
        assert!(streaming.contains("dprod pending"), "{streaming}");
        assert!(!streaming.contains("<pre>"), "{streaming}");

        let quoted = to_html("> ```product\n> {\"title\": oops}\n> ```");
        assert!(quoted.contains("<pre>"), "{quoted}");
        assert!(!quoted.contains("dprod"), "{quoted}");
    }

    #[test]
    fn a_reply_cannot_fake_a_row() {
        let html =
            to_html("x \u{E000}0\u{E000} <button data-copy=\"https://x.example\">y</button>");
        assert!(!html.contains("<button"), "{html}");
        assert!(!html.contains("data-copy"), "{html}");
        assert!(!html.contains(ROW_MARK), "{html}");
    }

    #[test]
    fn bad_latex_shows_as_code() {
        let html = to_html(r"see $\frac{1$ here");
        assert!(!html.contains("<math"), "{html}");
    }
}
