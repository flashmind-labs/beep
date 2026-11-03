//! A research's paper as a PDF. The writer's Markdown becomes Typst markup,
//! its `[n]` citations links to a numbered list of the sources, its LaTeX
//! math Typst's (through mitex), and Typst typesets it with the fonts it
//! bundles. Nothing is read from disk or the network.

use crate::store::SourceRow;
use anyhow::{Context, Result, bail};
use chrono::{Datelike, NaiveDate};
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use std::{fmt::Write, sync::LazyLock};
use tokio::{sync::Semaphore, task::spawn_blocking};
use typst::{
    Library, LibraryExt, World,
    diag::{FileError, FileResult},
    foundations::{Bytes, Datetime, Duration},
    syntax::{FileId, Source},
    text::{Font, FontBook},
    utils::LazyHash,
};
use typst_layout::PagedDocument;
use typst_pdf::PdfOptions;

/// Papers typeset at once; each takes a core for a second or two.
static RENDERS: Semaphore = Semaphore::const_new(2);
/// The largest PDF made, so it fits one reply.
pub const MAX_PDF: usize = 4 * 1024 * 1024;

/// What goes into the PDF.
pub struct Paper {
    /// The research's topic, the title when the draft has no `#` heading.
    pub topic: String,
    pub draft: String,
    pub sources: Vec<SourceRow>,
    pub date: NaiveDate,
}

/// A typeset paper.
pub struct Pdf {
    /// A file name, from the paper's title.
    pub name: String,
    pub data: Vec<u8>,
}

/// Typesets `paper`, off the async workers, a couple at a time.
pub async fn render(paper: Paper) -> Result<Pdf> {
    let _permit = RENDERS.acquire().await?;
    spawn_blocking(move || render_now(&paper))
        .await
        .context("the typesetting task died")?
}

fn render_now(paper: &Paper) -> Result<Pdf> {
    let markup = typst_markup(paper, Math::Typeset);
    let data = match typeset(markup.text, paper.date) {
        Ok(data) => data,
        // Math mitex could not turn into Typst's fails the whole document;
        // show it as LaTeX source instead.
        Err(error) if markup.has_math => {
            log::info!(
                "research pdf: typesetting the math failed, showing it as source: {error:#}"
            );
            typeset(typst_markup(paper, Math::Source).text, paper.date)?
        }
        Err(error) => return Err(error),
    };
    if data.len() > MAX_PDF {
        bail!("the PDF is too large");
    }
    Ok(Pdf {
        name: file_name(&markup.title),
        data,
    })
}

/// A name for the file, like "Solid-state batteries.pdf".
fn file_name(title: &str) -> String {
    let name: String = title
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || " -_,()".contains(c) {
                c
            } else {
                ' '
            }
        })
        .collect();
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    let name: String = name.chars().take(80).collect();
    format!("{}.pdf", if name.is_empty() { "Research" } else { &name })
}

/// How math is shown.
#[derive(Clone, Copy, PartialEq)]
enum Math {
    Typeset,
    /// As its LaTeX, in code type.
    Source,
}

struct Markup {
    text: String,
    title: String,
    has_math: bool,
}

/// Helpers mitex's output calls, as Typst functions. Anything else it
/// writes is plain Typst math.
const MITEX: &str = r#"#let mitexdisplay(it) = math.display(it)
#let mitexinline(it) = math.inline(it)
#let mitexbold(it) = math.bold(it)
#let mitexmathbf(it) = math.bold(math.upright(it))
#let mitexupright(it) = math.upright(it)
#let mitexitalic(it) = math.italic(it)
#let mitexcal(it) = math.cal(it)
#let mitexfrak(it) = math.frak(it)
#let mitexsans(it) = math.sans(it)
#let mitexmono(it) = math.mono(it)
#let mitexoverbrace(..args) = math.overbrace(..args)
#let mitexunderbrace(..args) = math.underbrace(..args)
#let mitexoverbracket(..args) = math.overbracket(..args)
#let mitexunderbracket(..args) = math.underbracket(..args)
#let mitexsqrt(..args) = {
  let a = args.pos()
  if a.len() > 1 { math.root(a.at(0), a.at(1)) } else { math.sqrt(a.at(0)) }
}
"#;

/// The page, type and headings.
const STYLE: &str = r##"#set page(paper: "a4", margin: (x: 2.3cm, y: 2.5cm), numbering: "1")
#set text(font: "Libertinus Serif", size: 11pt)
#set par(justify: true, leading: 0.68em, spacing: 1.1em)
#set heading(numbering: "1.")
#show heading: set block(above: 1.5em, below: 0.8em)
#show heading.where(level: 1): set text(size: 14pt)
#show link: set text(fill: rgb("#2a6e57"))
#show raw: set text(font: "DejaVu Sans Mono", size: 9pt)
#set table(stroke: 0.5pt + luma(170), inset: (x: 6pt, y: 5pt))
#show table.cell.where(y: 0): set text(weight: "bold")
#set quote(block: true)
"##;

fn typst_markup(paper: &Paper, math: Math) -> Markup {
    let mut writer = Writer::new(math, &paper.sources);
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_MATH
        | Options::ENABLE_TASKLISTS;
    for event in Parser::new_ext(&paper.draft, options) {
        writer.event(event);
    }
    writer.flush();
    let title = match writer.title.trim() {
        "" => paper.topic.trim().to_owned(),
        title => title.to_owned(),
    };

    let mut out = String::new();
    let _ = writeln!(
        out,
        "#set document(title: {}, author: \"Beep\")",
        string(&title)
    );
    out.push_str(STYLE);
    out.push_str(MITEX);
    let date = format!(
        "{} {} {}",
        paper.date.day(),
        paper.date.format("%B"),
        paper.date.year()
    );
    let _ = writeln!(
        out,
        "#align(center)[#text(size: 20pt, weight: \"bold\")[{}]\n#v(2pt)\n#text(size: 10pt, fill: luma(90))[Research by Beep, {}]]\n#v(14pt)\n",
        escape(&title),
        escape(&date)
    );
    out.push_str(&writer.out);
    if !paper.sources.is_empty() {
        out.push_str("\n#heading(numbering: none)[Sources]\n#set text(size: 9.5pt)\n#set par(justify: false)\n");
        for source in &paper.sources {
            let _ = writeln!(
                out,
                "#block(below: 0.9em)[#strong[{}.] {}. {}\\\n#emph[“{}”]] <src-{}>",
                source.number,
                escape(&source.title),
                link(&source.url, &escape(&source.url)),
                escape(&source.quote),
                source.number,
            );
        }
    }
    Markup {
        text: out,
        title,
        has_math: writer.has_math,
    }
}

/// A link when `url` is one a reader can open, else just `body`.
fn link(url: &str, body: &str) -> String {
    if url.starts_with("https://") || url.starts_with("http://") || url.starts_with("mailto:") {
        format!("#link({})[{}]", string(url), body)
    } else {
        body.to_owned()
    }
}

/// `text` as a Typst string literal.
fn string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `text` as Typst markup that shows it as is.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if "\\#$*_`<>@[]~/=-+.:'\"".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Turns the Markdown's events into Typst markup.
struct Writer<'a> {
    out: String,
    math: Math,
    sources: &'a [SourceRow],
    /// Text not written yet, so a citation split over events reads whole.
    text: String,
    /// The first `#` heading, which becomes the title.
    title: String,
    /// Inside the title heading.
    in_title: bool,
    seen_title: bool,
    /// Inside a code block: its language and text so far.
    code: Option<(String, String)>,
    /// Whether each open link was written as one.
    links: Vec<bool>,
    /// Inside an image: its alt text is all that shows.
    in_image: bool,
    has_math: bool,
}

impl<'a> Writer<'a> {
    fn new(math: Math, sources: &'a [SourceRow]) -> Self {
        Self {
            out: String::new(),
            math,
            sources,
            text: String::new(),
            title: String::new(),
            in_title: false,
            seen_title: false,
            code: None,
            links: Vec::new(),
            in_image: false,
            has_math: false,
        }
    }

    fn event(&mut self, event: Event) {
        if let Some((_, code)) = &mut self.code {
            match event {
                Event::Text(text) => code.push_str(&text),
                Event::End(TagEnd::CodeBlock) => {
                    let (lang, code) = self.code.take().unwrap_or_default();
                    let lang = match lang.as_str() {
                        "" => String::new(),
                        lang => format!(", lang: {}", string(lang)),
                    };
                    let _ = writeln!(
                        self.out,
                        "#raw(block: true{lang}, {})\n",
                        string(code.trim_end())
                    );
                }
                _ => {}
            }
            return;
        }
        if self.in_title {
            match event {
                Event::End(TagEnd::Heading(_)) => self.in_title = false,
                Event::Text(text) | Event::Code(text) => self.title.push_str(&text),
                _ => {}
            }
            return;
        }
        match event {
            Event::Text(text) => {
                self.text.push_str(&text);
                return;
            }
            Event::SoftBreak => {
                self.text.push(' ');
                return;
            }
            _ => {}
        }
        self.flush();
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Code(code) => {
                let _ = write!(self.out, "#raw({})", string(&code));
            }
            Event::InlineMath(latex) => self.math(&latex, false),
            Event::DisplayMath(latex) => self.math(&latex, true),
            Event::HardBreak => self.out.push_str("\\\n"),
            Event::Rule => self
                .out
                .push_str("\n#line(length: 100%, stroke: 0.5pt + luma(170))\n"),
            Event::TaskListMarker(done) => self.out.push_str(if done { "☑ " } else { "☐ " }),
            // Raw HTML shows as the text it is.
            Event::Html(html) | Event::InlineHtml(html) => self.out.push_str(&escape(&html)),
            Event::FootnoteReference(name) => self.out.push_str(&escape(&format!("[{name}]"))),
            Event::Text(_) | Event::SoftBreak => {}
        }
    }

    fn start(&mut self, tag: Tag) {
        match tag {
            Tag::Paragraph => {}
            Tag::Heading { level, .. } => {
                if level == HeadingLevel::H1 && !self.seen_title {
                    self.seen_title = true;
                    self.in_title = true;
                    return;
                }
                // The title takes `#`, so `##` makes the top sections.
                let level = match level {
                    HeadingLevel::H1 | HeadingLevel::H2 => 1,
                    HeadingLevel::H3 => 2,
                    HeadingLevel::H4 => 3,
                    _ => 4,
                };
                let _ = write!(self.out, "\n#heading(level: {level})[");
            }
            Tag::BlockQuote(_) => self.out.push_str("\n#quote[\n"),
            Tag::CodeBlock(kind) => {
                let lang = match kind {
                    CodeBlockKind::Fenced(lang) => lang
                        .split_whitespace()
                        .next()
                        .unwrap_or_default()
                        .to_owned(),
                    CodeBlockKind::Indented => String::new(),
                };
                self.code = Some((lang, String::new()));
            }
            Tag::List(None) => self.out.push_str("\n#list(\n"),
            Tag::List(Some(start)) => {
                let _ = write!(self.out, "\n#enum(start: {start},\n");
            }
            Tag::Item => self.out.push('['),
            Tag::Table(alignments) => {
                let _ = write!(self.out, "\n#table(columns: {},\n", alignments.len().max(1));
            }
            Tag::TableHead => self.out.push_str("table.header("),
            Tag::TableRow => {}
            Tag::TableCell => self.out.push('['),
            Tag::Emphasis => self.out.push_str("#emph["),
            Tag::Strong => self.out.push_str("#strong["),
            Tag::Strikethrough => self.out.push_str("#strike["),
            Tag::Link { dest_url, .. } => {
                let opened = link(&dest_url, "").ends_with("[]");
                if opened {
                    let _ = write!(self.out, "#link({})[", string(&dest_url));
                }
                self.links.push(opened);
            }
            Tag::Image { .. } => {
                self.in_image = true;
                self.out.push_str("#emph[");
            }
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => self.out.push_str("\n\n"),
            TagEnd::Heading(_) => self.out.push_str("]\n"),
            TagEnd::BlockQuote(_) => self.out.push_str("]\n"),
            TagEnd::List(_) => self.out.push_str(")\n"),
            TagEnd::Item => self.out.push_str("],\n"),
            TagEnd::Table => self.out.push_str(")\n"),
            TagEnd::TableHead => self.out.push_str("),\n"),
            TagEnd::TableRow => self.out.push('\n'),
            TagEnd::TableCell => self.out.push_str("], "),
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => self.out.push(']'),
            TagEnd::Link => {
                if self.links.pop().unwrap_or(false) {
                    self.out.push(']');
                }
            }
            TagEnd::Image => {
                self.in_image = false;
                self.out.push(']');
            }
            _ => {}
        }
    }

    fn math(&mut self, latex: &str, display: bool) {
        self.has_math = true;
        let typeset = match self.math {
            Math::Typeset => mitex::convert_math(latex, None).ok(),
            Math::Source => None,
        };
        match (typeset, display) {
            (Some(math), true) => {
                let _ = write!(self.out, "$ {math} $");
            }
            (Some(math), false) => {
                let _ = write!(self.out, "${math}$");
            }
            (None, true) => {
                let _ = write!(self.out, "\n#raw(block: true, {})\n", string(latex.trim()));
            }
            (None, false) => {
                let _ = write!(self.out, "#raw({})", string(latex.trim()));
            }
        }
    }

    /// Writes the pending text, its citations as links to the sources.
    fn flush(&mut self) {
        if self.text.is_empty() {
            return;
        }
        let text = std::mem::take(&mut self.text);
        let mut rest = text.as_str();
        while let Some(open) = rest.find('[') {
            let (before, from) = rest.split_at(open);
            let cited = from
                .find(']')
                .and_then(|close| Some((self.citation(&from[1..close])?, close)));
            match cited {
                // The citation sits right after the claim, as a superscript.
                Some((cited, close)) => {
                    self.out.push_str(&escape(before.trim_end_matches(' ')));
                    self.out.push_str(&cited);
                    rest = &from[close + 1..];
                }
                None => {
                    self.out.push_str(&escape(before));
                    self.out.push_str("\\[");
                    rest = &from[1..];
                }
            }
        }
        self.out.push_str(&escape(rest));
    }

    /// `3` or `2, 5` as superscript links to those sources, when each is
    /// one.
    fn citation(&self, inside: &str) -> Option<String> {
        let numbers: Vec<u32> = inside
            .split(',')
            .map(|n| n.trim().parse().ok())
            .collect::<Option<_>>()?;
        if numbers.is_empty()
            || !numbers
                .iter()
                .all(|n| self.sources.iter().any(|s| s.number == *n))
        {
            return None;
        }
        let links: Vec<String> = numbers
            .iter()
            .map(|n| format!("#link(<src-{n}>)[{n}]"))
            .collect();
        Some(format!("#super[{}]", links.join(", ")))
    }
}

/// The fonts Typst bundles, read once.
struct Fonts {
    book: LazyHash<FontBook>,
    fonts: Vec<Font>,
}

static FONTS: LazyLock<Fonts> = LazyLock::new(|| {
    let fonts: Vec<Font> = typst_kit::fonts::embedded().map(|(font, _)| font).collect();
    Fonts {
        book: LazyHash::new(FontBook::from_fonts(&fonts)),
        fonts,
    }
});

static LIBRARY: LazyLock<LazyHash<Library>> =
    LazyLock::new(|| LazyHash::new(Library::builder().build()));

/// One document with no files besides itself.
struct PaperWorld {
    source: Source,
    today: NaiveDate,
}

impl World for PaperWorld {
    fn library(&self) -> &LazyHash<Library> {
        &LIBRARY
    }
    fn book(&self) -> &LazyHash<FontBook> {
        &FONTS.book
    }
    fn main(&self) -> FileId {
        self.source.id()
    }
    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.source.id() {
            Ok(self.source.clone())
        } else {
            Err(FileError::AccessDenied)
        }
    }
    fn file(&self, _id: FileId) -> FileResult<Bytes> {
        Err(FileError::AccessDenied)
    }
    fn font(&self, index: usize) -> Option<Font> {
        FONTS.fonts.get(index).cloned()
    }
    fn today(&self, _offset: Option<Duration>) -> Option<Datetime> {
        let month = u8::try_from(self.today.month()).ok()?;
        let day = u8::try_from(self.today.day()).ok()?;
        Datetime::from_ymd(self.today.year(), month, day)
    }
}

fn typeset(markup: String, today: NaiveDate) -> Result<Vec<u8>> {
    let world = PaperWorld {
        source: Source::detached(markup),
        today,
    };
    let document: PagedDocument = typst::compile(&world)
        .output
        .map_err(|errors| anyhow::anyhow!("typesetting failed: {}", messages(&errors)))?;
    typst_pdf::pdf(&document, &PdfOptions::default())
        .map_err(|errors| anyhow::anyhow!("writing the PDF failed: {}", messages(&errors)))
}

fn messages(errors: &[typst::diag::SourceDiagnostic]) -> String {
    let messages: Vec<&str> = errors.iter().map(|e| e.message.as_str()).collect();
    messages.join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::research::Member;

    fn source(number: u32) -> SourceRow {
        SourceRow {
            number,
            url: format!("https://example.com/{number}"),
            title: format!("Report {number}"),
            quote: "It works.".to_owned(),
            member: Member::Scout(1),
        }
    }

    fn paper(draft: &str) -> Paper {
        Paper {
            topic: "Does it work?".to_owned(),
            draft: draft.to_owned(),
            sources: vec![source(1), source(2)],
            date: NaiveDate::from_ymd_opt(2026, 10, 27).expect("a date"),
        }
    }

    #[tokio::test]
    async fn a_paper_typesets_to_a_pdf() {
        let draft = "# Solid-state batteries\n\nProbably **not** by 2028 [1].\n\n\
            ## Cost\n\n| Maker | Year |\n|---|---|\n| A | 2027 |\n\n\
            - one *point* [2]\n- two `code`\n\n1. first\n2. second\n\n\
            Energy is $E = mc^2$ and\n\n$$\\frac{a}{b} + \\sqrt{x}$$\n\n\
            > A quote with a [link](https://example.com).\n\n```rust\nfn main() {}\n```\n";
        let pdf = render(paper(draft)).await.expect("the paper typesets");
        assert!(pdf.data.starts_with(b"%PDF"));
        assert!(
            typeset(
                typst_markup(&paper(draft), Math::Typeset).text,
                paper(draft).date
            )
            .is_ok(),
            "the math typesets"
        );
        assert_eq!(pdf.name, "Solid-state batteries.pdf");
    }

    #[tokio::test]
    async fn math_typst_cannot_read_still_typesets_as_source() {
        let pdf = render(paper("Odd $\\unknownmacro{x}{y}$ math, $$\\begin{weird}$$"))
            .await
            .expect("the paper still typesets");
        assert!(pdf.data.starts_with(b"%PDF"));
        assert_eq!(pdf.name, "Does it work.pdf");
    }

    #[test]
    fn markup_characters_are_escaped() {
        let markup = typst_markup(
            &paper("A #tag, $5 and *x* = y_1 <b> @me // no"),
            Math::Source,
        );
        assert!(!markup.text.contains(" #tag"));
        assert!(markup.text.contains("\\#tag"));
        assert!(markup.text.contains("\\@me"));
    }
}
