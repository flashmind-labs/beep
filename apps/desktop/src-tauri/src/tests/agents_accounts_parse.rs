use super::*;

const TOKEN: &str = "sk-ant-oat01-AbCdEfGhIjKlMnOpQrStUvWxYz0123456789_-AbCdEfGhIjKl";

#[test]
fn the_token_is_read_from_what_claude_drew() {
    let drawn = format!(
        "\u{1b}[32m\u{2713}\u{1b}[39m\u{1b}[1CLong-lived\u{1b}[1Cauthentication\u{1b}[1Ctoken\
         \u{1b}[1Ccreated\u{1b}[1Csuccessfully!\r\n\r\nYour OAuth token (valid for 1 year):\r\n\r\n\
         \u{1b}[33m{TOKEN}\u{1b}[39m\r\n\r\nStore this token securely.\r\n"
    );
    assert_eq!(token(&drawn).as_deref(), Some(TOKEN));
}

#[test]
fn a_wrapped_token_is_put_together() {
    let (head, tail) = TOKEN.split_at(30);
    let drawn = format!("Your OAuth token:\r\n{head}\r\n{tail}\r\n\r\nStore it.\r\n");
    assert_eq!(token(&drawn).as_deref(), Some(TOKEN));
}

#[test]
fn not_before_the_whole_token_came() {
    // The rest of the line may still be on its way.
    assert_eq!(token(&format!("Your OAuth token:\r\n{TOKEN}")), None);
    assert_eq!(token("Paste code here if prompted >"), None);
}

#[test]
fn an_end_without_a_token_says_the_last_line() {
    assert_eq!(
        ended_without_token("Welcome\r\n\u{1b}[31mOAuth error: denied\u{1b}[39m\r\n\r\n"),
        "Claude Code ended without a token: OAuth error: denied"
    );
    assert_eq!(
        ended_without_token(""),
        "Claude Code ended without a token. Paste one instead."
    );
}

#[test]
fn hyperlinks_and_moves_are_dropped() {
    let drawn = "\u{1b}]8;id=1;https://claude.com/cai/oauth\u{7}link\u{1b}]8;;\u{7}\u{1b}[2Ahi";
    assert_eq!(plain(drawn), "link\nhi");
}
