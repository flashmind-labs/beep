use super::*;

fn handed<'a>(name: &'a str, mime: &'a str, data: &'a [u8]) -> Handed<'a> {
    Handed { name, mime, data }
}

#[test]
fn long_text_is_cut() {
    let long = "x".repeat(MAX_INLINE + 10);
    let text = describe(&handed("big.txt", "text/plain", long.as_bytes()));
    assert!(text.contains("[Cut here."));
    assert!(text.len() < long.len() + 300);
}

#[test]
fn names_cannot_break_out_of_the_tag() {
    let text = describe(&handed("a\"><b.txt", "text/plain", b"hi"));
    assert!(text.starts_with("<file name=\"a&quot;>&lt;b.txt\""));
}
