use super::*;

const KEY: &str = "fake-api-key-0123456789";

fn config(address: &str) -> String {
    format!(
        r#"<configuration version="37">
    <folder id="abcd-1234" label="Notes" path="~/notes" type="sendreceive"></folder>
    <device id="AAAAAAA-BBBBBBB" name="build-box"></device>
    <gui enabled="true" tls="false" debugging="false">
        <address>{address}</address>
        <apikey>{KEY}</apikey>
        <theme>default</theme>
    </gui>
</configuration>"#
    )
}

#[test]
fn the_config_gives_the_gui_and_key_and_never_prints_the_key() {
    let gui = parse_gui(&config("0.0.0.0:8384")).unwrap();
    assert_eq!(gui.addr, "127.0.0.1:8384");
    assert_eq!(gui.key.expose_secret(), KEY);
    assert!(!format!("{gui:?}").contains(KEY));

    let tls = config("127.0.0.1:8384").replace(r#"tls="false""#, r#"tls="true""#);
    assert!(parse_gui(&tls).is_err());
    assert!(parse_gui("<configuration></configuration>").is_err());
    assert!(parse_gui("not xml").is_err());
}

#[test]
fn gui_addresses_are_reached_on_loopback() {
    assert_eq!(gui_addr("[::]:8384", false).unwrap(), "[::1]:8384");
    assert_eq!(gui_addr(":8384", false).unwrap(), "127.0.0.1:8384");
    assert_eq!(
        gui_addr("http://localhost:8384/", false).unwrap(),
        "localhost:8384"
    );
    assert!(gui_addr("https://127.0.0.1:8384", false).is_err());
    assert!(gui_addr("unix:///run/syncthing.sock", false).is_err());
}
