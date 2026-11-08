use super::*;

fn notice() -> Notice {
    Notice {
        title: "Done".into(),
        body: "The report is ready.".into(),
        session_id: "s1".into(),
        app: None,
        approval: None,
        changed: false,
        group: false,
    }
}

#[test]
fn a_sealed_notice_opens_on_its_phone_only() {
    let phone = SecretKey::from_bytes(&[1; 32]);
    let other = SecretKey::from_bytes(&[2; 32]);
    let sealed = seal(&phone.public(), &notice()).unwrap();
    assert_eq!(open(&phone, &sealed).unwrap(), notice());
    assert!(open(&other, &sealed).is_err());
    let short = STANDARD.encode([9; POINT_LEN + TAG_LEN - 1]);
    assert!(format!("{:#}", open(&phone, &short).unwrap_err()).contains("too short"));
    // A fresh one-off key each time.
    assert_ne!(seal(&phone.public(), &notice()).unwrap(), sealed);
}
