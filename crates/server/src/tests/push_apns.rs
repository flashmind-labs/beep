use super::*;
use crate::protocol::seal;
use iroh::SecretKey;

fn notice(title: &str, changed: bool) -> Notice {
    Notice {
        title: title.into(),
        body: "The report is ready.".into(),
        session_id: "s1".into(),
        app: None,
        approval: None,
        changed,
        group: false,
    }
}

#[test]
fn a_notice_goes_sealed_behind_a_placeholder() {
    let phone = SecretKey::from_bytes(&[1; 32]);
    let push = Push::notice(&phone.public(), &notice("Done", false))
        .unwrap()
        .unwrap();
    assert!(!push.background);
    assert_eq!(push.payload["aps"]["mutable-content"], 1);
    assert_eq!(push.payload["aps"]["alert"]["body"], PLACEHOLDER_BODY);
    let text = push.payload.to_string();
    assert!(!text.contains("report"), "the notice's text is sealed");
    let sealed = push.payload["notice"].as_str().unwrap();
    assert_eq!(seal::open(&phone, sealed).unwrap(), notice("Done", false));
}

#[test]
fn a_long_notice_still_fits_a_push() {
    let phone = SecretKey::from_bytes(&[1; 32]).public();
    let mut long = notice("Done", false);
    long.body = "é".repeat(5000);
    let push = Push::notice(&phone, &long).unwrap().unwrap();
    assert!(push.payload.to_string().len() <= 4096);
}

/// Checks a real key against Apple, named by the `BEEP_APNS_*` env vars.
#[tokio::test]
#[ignore = "talks to Apple"]
async fn apple_takes_the_key() {
    let var = |name| std::env::var(name).unwrap_or_else(|_| panic!("{name} is set"));
    let apns = Apns::new(&ApnsConfig {
        key_file: var("BEEP_APNS_KEY_FILE").into(),
        key_id: var("BEEP_APNS_KEY_ID"),
        team_id: var("BEEP_APNS_TEAM_ID"),
        topic: var("BEEP_APNS_TOPIC"),
    })
    .unwrap();
    let token = "00".repeat(32);

    for environment in [Environment::Sandbox, Environment::Production] {
        let delivery = apns.wake(&token, environment).await.unwrap();
        assert_eq!(delivery, Delivery::Gone, "{environment:?}");
    }
}
