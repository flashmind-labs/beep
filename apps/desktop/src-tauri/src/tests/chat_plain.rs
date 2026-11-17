use super::*;

#[test]
fn replies_keep_the_source_and_line_breaks_as_text() {
    assert_eq!(
        to_html("# Title\n**words** and $x$"),
        "<div class=\"plain-reply\"># Title\n**words** and $x$</div>"
    );
}

#[test]
fn untrusted_markup_cannot_create_elements_or_attributes() {
    let source = "</div><img src=x onerror=alert(1)><script>bad()</script>&\"";

    for html in [to_html(source), user_html(source), untrusted_html(source)] {
        assert!(!html.contains("<img"));
        assert!(!html.contains("<script"));
        assert!(html.contains("&lt;/div&gt;"));
        assert!(html.contains("&amp;&quot;"));
    }
}

#[test]
fn previews_are_single_line_and_escaped() {
    assert_eq!(plain_text(" a\n\n b\t c "), "a b c");
    assert_eq!(inline_html("<tag>\n & words"), "&lt;tag&gt; &amp; words");
}
