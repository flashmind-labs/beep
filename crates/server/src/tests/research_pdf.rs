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
