//! Safe, unformatted replies for builds without rich text.

use pulldown_cmark_escape::escape_html;

pub fn to_html(text: &str) -> String {
    format!("<div class=\"plain-reply\">{}</div>", escaped(text))
}

pub fn inline_html(text: &str) -> String {
    escaped(&plain_text(text))
}

pub fn plain_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn untrusted_html(text: &str) -> String {
    to_html(text)
}

pub fn user_html(text: &str) -> String {
    to_html(text)
}

fn escaped(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    // The library's String sink only appends text; its writes cannot fail.
    escape_html(&mut out, text).expect("escaping into a String cannot fail");
    out
}

#[cfg(test)]
#[path = "../tests/chat_plain.rs"]
mod tests;
