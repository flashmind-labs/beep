//! Replies arrive as Markdown; the bar shows them as HTML cleaned of
//! anything that could run.

use pulldown_cmark::{Options, Parser, html};

pub fn to_html(markdown: &str) -> String {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_GFM;
    let mut out = String::with_capacity(markdown.len() * 3 / 2);
    html::push_html(&mut out, Parser::new_ext(markdown, options));
    ammonia::clean(&out)
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
}
