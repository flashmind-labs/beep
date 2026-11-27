#[test]
fn mermaid_waits_for_its_closing_fence() {
    for fence in ["```", "~~~~"] {
        let block = format!("{fence}mermaid\nflowchart LR\n A-->B\n");
        assert!(super::to_html(&block).contains("language-mermaid-pending"));
        let html = super::to_html(&format!("{block}{fence}\n"));
        assert!(html.contains("class=\"language-mermaid\""), "{html}");
        assert!(html.contains("A--&gt;B"), "{html}");
    }
}

#[test]
fn untrusted_markdown_shows_raw_html_as_text() {
    let html = super::untrusted_html(
        "**hi** <script>alert(1)</script>\n\n![x](https://example.com/t.png) <img src=x onerror=alert(1)>",
    );
    assert!(html.contains("<strong>hi</strong>"), "{html}");
    assert!(html.contains("&lt;script&gt;"), "{html}");
    assert!(!html.contains("<img"), "{html}");
}

use super::*;

#[test]
fn user_message_preserves_line_breaks_and_code() {
    let html = user_html("{thought}\n{tools collapsed}\n`inline`\n\n```text\nfirst\nsecond\n```");
    assert!(
        html.contains("{thought}<br>\n{tools collapsed}<br>\n<code>inline</code>"),
        "{html}"
    );
    assert!(html.contains("first\nsecond\n</code>"), "{html}");
}

#[test]
fn a_users_message_is_markdown_with_raw_html_as_text() {
    let html = user_html(
        "# Review on ~/app\n\n### src/a.rs:3\n\nUse `limit` <script>x()</script> ![t](https://example.com/t.png)\n\nSee https://example.com/1",
    );
    assert!(html.contains("<h1>Review on ~/app</h1>"), "{html}");
    assert!(html.contains("<h3>src/a.rs:3</h3>"), "{html}");
    assert!(html.contains("<code>limit</code>"), "{html}");
    assert!(html.contains("&lt;script&gt;"), "{html}");
    assert!(
        !html.contains("<script") && !html.contains("<img"),
        "{html}"
    );
    assert!(
        html.contains(r#"<a href="https://example.com/1""#),
        "{html}"
    );
}

#[test]
fn a_pasted_block_folds_and_stays_text() {
    let html = user_html(
        "look at this\n<pasted_content id=\"1\">\n# not a heading\n<b>x</b>\n</pasted_content id=\"1\">\nand *this*\u{E001}0\u{E001}",
    );
    assert!(
            html.contains(r#"<details class="paste" data-paste="0"><summary>Pasted text · 2 lines</summary><pre># not a heading"#),
            "{html}"
        );
    assert!(
        html.contains("&lt;b&gt;x&lt;/b&gt;</pre></details>"),
        "{html}"
    );
    assert!(html.contains("<em>this</em>"), "{html}");
    assert_eq!(html.matches("<details").count(), 1, "{html}");
    assert!(!html.contains('\u{E001}'), "{html}");
}

#[test]
fn renders_markdown_and_drops_scripts() {
    let html = to_html("**hi** <script>alert(1)</script> [x](javascript:alert(1))");
    assert!(html.contains("<strong>hi</strong>"));
    assert!(!html.contains("<script"), "{html}");
    assert!(!html.contains("javascript:"));
}

#[test]
fn a_link_to_an_ipv6_address_stays_a_link() {
    let html = to_html("[docs](http://[fd00::1]:8080/a%5Db/) and [b](https://[fd00::2]/)");
    assert!(
        html.contains(r#"href="http://[fd00::1]:8080/a%5Db/""#),
        "{html}"
    );
    assert!(html.contains(r#"href="https://[fd00::2]/""#), "{html}");
}

#[test]
fn links_bare_addresses() {
    let html = to_html(
        "Join at https://zoom.us/j/9611?pwd=9k1o_HI.1. (see https://en.wikipedia.org/wiki/Foo_(bar))",
    );
    assert!(
            html.contains(r#"<a href="https://zoom.us/j/9611?pwd=9k1o_HI.1" rel="noopener noreferrer">https://zoom.us/j/9611?pwd=9k1o_HI.1</a>. "#),
            "{html}"
        );
    assert!(
        html.contains(r#">https://en.wikipedia.org/wiki/Foo_(bar)</a>)"#),
        "{html}"
    );
    let html =
        to_html("[site](https://a.com) and `see https://b.com`\n\n```sh\ncurl https://c.com\n```");
    assert_eq!(html.matches("<a ").count(), 1, "{html}");
}

#[test]
fn links_an_address_in_inline_code() {
    let url = "http://[2001:db8::1]:8791/desktop/index.html#board";
    let linked = format!(r#"<a href="{url}" rel="noopener noreferrer"><code>{url}</code></a>"#);

    for html in [
        to_html(&format!("Open `{url}`.")),
        user_html(&format!("Open `{url}`.")),
    ] {
        assert!(html.contains(&linked), "{html}");
    }

    // A code block that holds one address, and nothing else, links it too.
    for block in [format!("```\n{url}\n```"), format!("```text\n{url}\n```")] {
        let html = to_html(&block);
        assert!(
            html.contains(&format!(r#"<a href="{url}" rel="noopener noreferrer">"#)),
            "{html}"
        );
    }
    let html = to_html(&format!("```\n{url}\n{url}\n```"));
    assert!(!html.contains("<a "), "{html}");

    let html = to_html(&format!("Open {url}."));
    assert!(
        html.contains(&format!(
            r#"<a href="{url}" rel="noopener noreferrer">{url}</a>."#
        )),
        "{html}"
    );
    let html = to_html("[`https://a.com`](https://b.com)");
    assert_eq!(html.matches("<a ").count(), 1, "{html}");
}

#[test]
fn links_beep_session_urls_in_markdown_and_plain_text() {
    let md = to_html("[Main work](beep://session/session_abc123@build-box)");
    assert!(
        md.contains(r#"<a href="beep://session/session_abc123@build-box" rel="noopener noreferrer">Main work</a>"#),
        "{md}"
    );

    let bare = to_html(
        "Open beep://session/00000000-0000-4000-8000-000000000042. (See beep://session/abc@box)",
    );
    assert!(
        bare.contains(r#"<a href="beep://session/00000000-0000-4000-8000-000000000042" rel="noopener noreferrer">beep://session/00000000-0000-4000-8000-000000000042</a>."#),
        "{bare}"
    );
    assert!(
        bare.contains(r#"<a href="beep://session/abc@box" rel="noopener noreferrer">beep://session/abc@box</a>)"#),
        "{bare}"
    );

    let code = to_html("Run `beep://session/s1@box`.");
    assert!(
        code.contains(r#"<a href="beep://session/s1@box" rel="noopener noreferrer"><code>beep://session/s1@box</code></a>."#),
        "{code}"
    );

    let user = user_html("Check [old chat](beep://session/s2) and beep://session/s3.");
    assert!(user.contains(r#"href="beep://session/s2""#), "{user}");
    assert!(user.contains(r#"href="beep://session/s3""#), "{user}");

    // Untrusted external markdown (e.g. GitHub issues) does not keep beep:// links.
    let untrusted = untrusted_html("[x](beep://session/s1)");
    assert!(!untrusted.contains("beep://"), "{untrusted}");

    // Bare scheme prefix with no id stays plain text.
    let empty = to_html("Use beep://session/ to link.");
    assert!(!empty.contains("<a "), "{empty}");
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
fn a_reply_cannot_add_a_button() {
    let html = to_html("x <button data-copy=\"https://x.example\">y</button>");
    assert!(!html.contains("<button"), "{html}");
    assert!(html.contains("&lt;button"), "{html}");
}

#[test]
fn bad_latex_shows_as_code() {
    let html = to_html(r"see $\frac{1$ here");
    assert!(!html.contains("<math"), "{html}");
}

#[test]
fn a_notification_reply_renders_inline_without_html_or_links() {
    let reply = "<br>\n\nMerged into **main** as `b09a9be`.\n\n- see [the PR](https://example.com)\n- <script>x</script>";
    assert_eq!(
        inline_html(reply),
        "Merged into <strong>main</strong> as <code>b09a9be</code>. see the PR"
    );
    assert_eq!(plain_text(reply), "Merged into main as b09a9be. see the PR");
}

#[test]
fn an_unknown_tag_shows_as_text_and_keeps_the_rest() {
    let html = to_html("a `Note: {x}` note named <title>. Then **bold** and <kbd>K</kbd>.");
    assert!(html.contains("&lt;title&gt;"), "{html}");
    assert!(html.contains("<strong>bold</strong>"), "{html}");
    assert!(html.contains("<kbd>K</kbd>"), "{html}");
}

#[test]
fn a_known_html_block_still_renders() {
    let html = to_html("<details>\n<summary>More</summary>\n\nhidden\n\n</details>");
    assert!(html.contains("<details>"), "{html}");
    assert!(html.contains("<summary>More</summary>"), "{html}");
}
