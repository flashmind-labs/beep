use super::*;

#[test]
fn common_keys_have_codes() {
    assert_eq!(code("t"), Some(0x11));
    assert_eq!(code("return"), Some(0x24));
    assert_eq!(code("left"), Some(0x7B));
    assert_eq!(code("f5"), Some(0x60));
    assert_eq!(code("hyper"), None);
}

#[test]
fn every_letter_and_digit_has_a_code() {
    for c in ('a'..='z').chain('0'..='9') {
        assert!(code(&c.to_string()).is_some(), "{c}");
    }
}
