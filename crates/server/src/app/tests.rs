use super::*;
use crate::protocol::Notice;
use iroh::{EndpointId, SecretKey};

fn key(n: u8) -> EndpointId {
    SecretKey::from_bytes(&[n; 32]).public()
}

fn titled(title: &str) -> Notice {
    Notice {
        title: title.into(),
        ..UserNotice::changed("u1", "").notice
    }
}

#[test]
fn a_notice_reaches_only_the_devices_it_names() {
    let (phone, mac, other_mac) = (key(1), key(2), key(3));
    let (is_phone, is_mac) = (DeviceKind::Phone {}, DeviceKind::Desktop {});
    let mut sent = UserNotice {
        user: "u1".into(),
        notice: titled("plain"),
        desktop: Some(titled("for computers")),
        to: Reach::Everyone,
    };
    let got = |sent: &UserNotice, id, kind| sent.for_device(id, kind).map(|n| n.title.clone());
    assert_eq!(got(&sent, &phone, &is_phone).as_deref(), Some("plain"));
    assert_eq!(got(&sent, &mac, &is_mac).as_deref(), Some("for computers"));

    sent.to = Reach::Desktops;
    assert_eq!(got(&sent, &phone, &is_phone), None, "never a phone");
    assert_eq!(got(&sent, &mac, &is_mac).as_deref(), Some("for computers"));
    assert_eq!(
        got(&sent, &other_mac, &is_mac).as_deref(),
        Some("for computers")
    );

    sent.to = Reach::Device(phone);
    assert_eq!(got(&sent, &phone, &is_phone).as_deref(), Some("plain"));
    assert_eq!(got(&sent, &mac, &is_mac), None);

    sent.to = Reach::Device(mac);
    assert_eq!(got(&sent, &phone, &is_phone), None);
    assert_eq!(got(&sent, &mac, &is_mac).as_deref(), Some("for computers"));
    assert_eq!(
        got(&sent, &other_mac, &is_mac),
        None,
        "only the Mac that started it"
    );
}
