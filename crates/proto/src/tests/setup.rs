use super::*;

fn server() -> EndpointId {
    SecretKey::from_bytes(&[4; 32]).public()
}

#[test]
fn a_token_survives_its_text() {
    let token = SetupToken::generate(server());
    let text = token.to_string();
    assert!(text.starts_with("bk1-"), "{text}");
    assert_eq!(text.len(), 4 + TEXT_LEN + TEXT_LEN / GROUP - 1);
    assert_eq!(SetupToken::parse(&text).unwrap(), token);
    // Pasted in capitals, without dashes or with a line break.
    let rest = text[4..].to_uppercase().replacen('-', "", 5);
    let loose = format!(" BK1-{}\n", rest.replace('-', "\n"));
    assert_eq!(SetupToken::parse(&loose).unwrap(), token);
    assert_eq!(
        token.command(),
        format!("beep-node setup {text}"),
        "the command is the token's"
    );
}

#[test]
fn two_tokens_differ_and_hash_apart() {
    let a = SetupToken::generate(server());
    let b = SetupToken::generate(server());
    assert_ne!(a, b);
    assert_ne!(a.hash(), b.hash());
    assert_eq!(a.hash().len(), 64);
    assert!(!a.hash().contains(&a.to_string()[4..]));
    assert!(!format!("{a:?}").contains(&a.to_string()[60..]));
}

#[test]
fn a_mistyped_token_is_caught() {
    let text = SetupToken::generate(server()).to_string();
    let mut chars: Vec<char> = text.chars().collect();
    let i = 20;
    chars[i] = if chars[i] == 'a' { 'b' } else { 'a' };
    let typo: String = chars.into_iter().collect();
    assert_eq!(SetupToken::parse(&typo), Err(TokenError::BadCheck));
    assert_eq!(
        SetupToken::parse(&text[..text.len() - 1]),
        Err(TokenError::Malformed)
    );
    assert_eq!(
        SetupToken::parse("beep-link:abc"),
        Err(TokenError::Malformed)
    );
    assert_eq!(SetupToken::parse("bk1-u!"), Err(TokenError::Malformed));
    assert_eq!(SetupToken::parse("bk"), Err(TokenError::Malformed));
}
