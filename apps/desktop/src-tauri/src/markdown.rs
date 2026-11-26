//! Replies arrive as Markdown; the bar shows them as HTML cleaned of
//! anything that could run. Math in `$x$`, `$$x$$`, `\(x\)` or `\[x\]`
//! becomes MathML, which WebKit draws natively.

use std::ops::Range;

use pulldown_cmark::{
    CodeBlockKind, CowStr, Event, LinkType, Options, Parser, Tag, TagEnd, TextMergeStream, html,
};
use pulldown_latex::{ParserError, RenderConfig, Storage, config::DisplayMode, push_mathml};

// Stands in for a pasted block in a user's message until the HTML is
// cleaned. A private-use character, removed from the message first so it
// cannot fake one.
const PASTE_MARK: char = '\u{E001}';

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

/// What markdown from anyone parses with; the user's own also reads math.
const PLAIN_OPTIONS: Options = Options::ENABLE_TABLES
    .union(Options::ENABLE_STRIKETHROUGH)
    .union(Options::ENABLE_TASKLISTS)
    .union(Options::ENABLE_GFM);

pub fn to_html(markdown: &str) -> String {
    let options = PLAIN_OPTIONS.union(Options::ENABLE_MATH);
    let source = dollar_delimiters(markdown);
    let events = Parser::new_ext(&source, options)
        .into_offset_iter()
        .map(|(event, range)| match event {
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(lang)))
                if lang.trim() == "mermaid" && !closed(&source[range]) =>
            {
                Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(
                    "mermaid-pending".into(),
                )))
            }
            other => other,
        });
    let events = autolink(TextMergeStream::new(events))
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
    let out = restore_ip_hosts(out);
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

/// Markdown as one run of inline HTML for a short preview, such as a
/// notification's reply: emphasis and code stay, raw HTML and links go,
/// and breaks and blocks become spaces.
pub fn inline_html(markdown: &str) -> String {
    let mut out = String::with_capacity(markdown.len() * 3 / 2);
    html::push_html(&mut out, inline(markdown));

    ammonia::Builder::default()
        .rm_tags(["img"])
        .clean(out.trim())
        .to_string()
}

/// Markdown as plain text on one line, for a system notification.
pub fn plain_text(markdown: &str) -> String {
    let mut text = String::with_capacity(markdown.len());

    for event in inline(markdown) {
        if let Event::Text(part) | Event::Code(part) = event {
            text.push_str(&part);
        }
    }

    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The inline events of `markdown`, with a space for each break and block.
fn inline(markdown: &str) -> impl Iterator<Item = Event<'_>> {
    Parser::new_ext(markdown, PLAIN_OPTIONS).filter_map(|event| match event {
        Event::Text(_)
        | Event::Code(_)
        | Event::Start(Tag::Emphasis | Tag::Strong | Tag::Strikethrough)
        | Event::End(TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough) => Some(event),
        Event::SoftBreak | Event::HardBreak | Event::Rule | Event::End(_) => {
            Some(Event::Text(" ".into()))
        }
        _ => None,
    })
}

/// Markdown someone else wrote, such as an issue on a code host, as HTML.
/// Raw HTML in it shows as text and images are left out, so nothing in it
/// runs or loads.
pub fn untrusted_html(markdown: &str) -> String {
    let events = Parser::new_ext(markdown, PLAIN_OPTIONS).map(|event| match event {
        Event::Html(raw) | Event::InlineHtml(raw) => Event::Text(raw),
        other => other,
    });
    let mut out = String::with_capacity(markdown.len() * 3 / 2);
    html::push_html(&mut out, events);
    ammonia::Builder::default()
        .rm_tags(["img"])
        .clean(&out)
        .to_string()
}

/// What the user wrote to a coding agent, as HTML. As in `untrusted_html`,
/// raw HTML shows as text and images are left out, since a review's
/// comments may come from an agent. Bare addresses become links, and each
/// block a terminal pasted in (`<pasted_content id="..">`) folds into
/// "Pasted text · 8 lines", numbered by `data-paste` for the page to open.
pub fn user_html(markdown: &str) -> String {
    let (source, pastes) = pasted_blocks(&markdown.replace(PASTE_MARK, ""));
    let events = Parser::new_ext(&source, PLAIN_OPTIONS).map(|event| match event {
        Event::Html(raw) | Event::InlineHtml(raw) => Event::Text(raw),
        Event::SoftBreak => Event::HardBreak,
        other => other,
    });
    let mut out = String::with_capacity(source.len() * 3 / 2);
    html::push_html(&mut out, autolink(TextMergeStream::new(events)).into_iter());
    let out = restore_ip_hosts(out);
    let mut clean = ammonia::Builder::default()
        .rm_tags(["img"])
        .clean(&out)
        .to_string();
    // The folds are built from escaped text, so they go in after cleaning.
    for (i, body) in pastes.iter().enumerate() {
        let lines = body.split('\n').count();
        let mut text = String::with_capacity(body.len());
        html::push_html(
            &mut text,
            std::iter::once(Event::Text(body.as_str().into())),
        );
        let fold = format!(
            r#"<details class="paste" data-paste="{i}"><summary>Pasted text · {lines} {}</summary><pre>{text}</pre></details>"#,
            if lines == 1 { "line" } else { "lines" },
        );
        let mark = paste_mark(i);
        let own_paragraph = format!("<p>{mark}</p>");
        clean = if clean.contains(&own_paragraph) {
            clean.replacen(&own_paragraph, &fold, 1)
        } else {
            clean.replacen(&mark, &fold, 1)
        };
    }

    clean
}

fn paste_mark(i: usize) -> String {
    format!("{PASTE_MARK}{i}{PASTE_MARK}")
}

/// `text` with each pasted block (`<pasted_content id="x">..</pasted_content
/// id="x">`) put by a paragraph of its own holding its mark, and the blocks.
fn pasted_blocks(text: &str) -> (String, Vec<String>) {
    const OPEN: &str = "<pasted_content id=\"";
    let mut out = String::with_capacity(text.len());
    let mut pastes = Vec::new();
    let mut rest = text;

    while let Some(at) = rest.find(OPEN) {
        let after = &rest[at + OPEN.len()..];
        let Some(id_end) = after.find("\">") else {
            break;
        };
        let id = &after[..id_end];
        let body = &after[id_end + 2..];
        let close = format!("</pasted_content id=\"{id}\">");
        let Some(end) = body.find(&close) else {
            break;
        };
        let inner = &body[..end];
        let inner = inner.strip_prefix('\n').unwrap_or(inner);
        let inner = inner.strip_suffix('\n').unwrap_or(inner);
        out.push_str(&rest[..at]);
        out.push_str(&format!("\n\n{}\n\n", paste_mark(pastes.len())));
        pastes.push(inner.to_owned());
        rest = &body[end + close.len()..];
    }

    out.push_str(rest);
    (out, pastes)
}

/// The start of a link to `dest_url`.
fn link(dest_url: CowStr<'_>) -> Event<'_> {
    Event::Start(Tag::Link {
        link_type: LinkType::Autolink,
        dest_url,
        title: CowStr::Borrowed(""),
        id: CowStr::Borrowed(""),
    })
}

/// Turns bare `http(s)://` addresses in text into links, as GitHub does,
/// and inline code or a plain code block that is one address into a link
/// around that code. Other text inside links, code blocks and HTML stays as
/// it is.
fn autolink<'a>(events: impl Iterator<Item = Event<'a>>) -> Vec<Event<'a>> {
    let mut out = Vec::new();
    let mut in_link = 0usize;
    let mut in_code = false;
    // In a code block with no language, or a plain text one.
    let mut in_plain_code = false;

    for event in events {
        match &event {
            Event::Start(Tag::Link { .. }) => in_link += 1,
            Event::End(TagEnd::Link) => in_link = in_link.saturating_sub(1),
            Event::Start(Tag::CodeBlock(kind)) => {
                in_code = true;
                in_plain_code = match kind {
                    CodeBlockKind::Indented => true,
                    CodeBlockKind::Fenced(lang) => {
                        matches!(lang.trim(), "" | "text" | "txt" | "plain" | "url")
                    }
                };
            }
            Event::End(TagEnd::CodeBlock) => {
                in_code = false;
                in_plain_code = false;
            }
            Event::Text(text)
                if in_plain_code && find_url(text.trim()) == Some(0..text.trim().len()) =>
            {
                out.push(link(text.trim().to_owned().into()));
                out.push(event);
                out.push(Event::End(TagEnd::Link));
                continue;
            }
            Event::Code(code) if in_link == 0 && find_url(code) == Some(0..code.len()) => {
                out.push(link(code.clone()));
                out.push(event);
                out.push(Event::End(TagEnd::Link));
                continue;
            }
            Event::Text(text) if in_link == 0 && !in_code => {
                let mut rest = text.as_ref();

                if find_url(rest).is_none() {
                    out.push(event);
                    continue;
                }

                while let Some(found) = find_url(rest) {
                    let url = &rest[found.clone()];
                    if found.start > 0 {
                        out.push(Event::Text(rest[..found.start].to_owned().into()));
                    }
                    out.push(link(url.to_owned().into()));
                    out.push(Event::Text(url.to_owned().into()));
                    out.push(Event::End(TagEnd::Link));
                    rest = &rest[found.end..];
                }

                if !rest.is_empty() {
                    out.push(Event::Text(rest.to_owned().into()));
                }
                continue;
            }
            _ => {}
        }

        out.push(event);
    }

    out
}

/// Puts back the brackets of an IPv6 host in links. The renderer escapes them
/// (`http://%5B::1%5D/`), and the cleaner drops an `href` whose host no longer
/// parses.
fn restore_ip_hosts(html: String) -> String {
    if !html.contains("://%5B") {
        return html;
    }

    let mut out = String::with_capacity(html.len());
    let mut rest = html.as_str();

    while let Some(at) = ["href=\"http://%5B", "href=\"https://%5B"]
        .iter()
        .filter_map(|open| rest.find(open).map(|at| at + open.len()))
        .min()
    {
        let (before, after) = rest.split_at(at);
        out.push_str(&before[..before.len() - "%5B".len()]);
        out.push('[');
        rest = after;
        // Only the closing bracket of this link's host.
        let end = rest.find('"').unwrap_or(rest.len());

        if let Some(close) = rest[..end].find("%5D") {
            out.push_str(&rest[..close]);
            out.push(']');
            rest = &rest[close + "%5D".len()..];
        }
    }

    out.push_str(rest);
    out
}

/// Where the first bare web address in `text` is. Trailing punctuation
/// stays out, and so does a closing bracket the address did not open.
fn find_url(text: &str) -> Option<Range<usize>> {
    let start = ["https://", "http://"]
        .iter()
        .filter_map(|scheme| text.find(scheme))
        .min()?;
    let len = text[start..]
        .find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"' | '`'))
        .unwrap_or(text.len() - start);
    let mut url = &text[start..start + len];

    loop {
        let trimmed = url.trim_end_matches(['.', ',', ';', ':', '!', '?', '\'', '*', '_']);
        let trimmed = match trimmed.strip_suffix(')') {
            Some(inner) if inner.matches('(').count() < trimmed.matches(')').count() => inner,
            _ => trimmed,
        };

        if trimmed == url {
            break;
        }
        url = trimmed;
    }

    let scheme_only = url.ends_with("://");

    if scheme_only {
        let next = start + url.len();
        return find_url(&text[next..]).map(|r| r.start + next..r.end + next);
    }

    Some(start..start + url.len())
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
#[path = "tests/markdown.rs"]
mod tests;
