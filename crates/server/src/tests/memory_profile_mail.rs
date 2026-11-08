use super::*;

#[test]
fn senders_from_dating_and_medical_services_are_skipped() {
    assert!(skipped("Tinder <no-reply@gotinder.com>"));
    assert!(skipped("MyChart <noreply@mychart.org>"));
    assert!(!skipped("Zara <news@zara.com>"));
    assert_eq!(address("\"Ana\" <Ana@Example.com>"), "ana@example.com");
    assert_eq!(sender_name("Zara <news@zara.com>"), "Zara");
    assert_eq!(sender_name("news@zara.com"), "zara.com");
}
