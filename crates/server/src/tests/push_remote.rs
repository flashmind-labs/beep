use super::*;
use iroh::SecretKey;

const MAX: usize = 4000;

fn notice(body: String) -> Notice {
    Notice {
        title: "Done".into(),
        body,
        session_id: "s1".into(),
        app: None,
        approval: None,
        changed: false,
        group: false,
    }
}

#[test]
fn a_long_body_is_cut_to_fit_a_push() {
    let phone = SecretKey::from_bytes(&[1; 32]);
    let long = "é".repeat(5000);
    let sealed = seal_to_fit(&phone.public(), &notice(long.clone()), MAX)
        .unwrap()
        .expect("a body can always be cut");
    assert!(sealed.len() <= MAX);
    let opened = seal::open(&phone, &sealed).unwrap();
    assert!(opened.body.ends_with("..."));
    assert!(long.starts_with(opened.body.trim_end_matches("...")));
    assert!(opened.body.len() > 2000);
}
