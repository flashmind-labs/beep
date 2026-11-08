use super::*;

fn config(text: &str) -> InstanceConfig {
    toml::from_str(text).expect("a valid instance table")
}

#[test]
fn retired_keys_still_load() {
    let instance = Instance::new(config(
        "agents = false\nbackground = false\nevents = false\nnodes = false",
    ));
    assert_eq!(instance.name(), "Beep");
}

#[test]
fn the_link_code_names_this_server() {
    let key = SecretKey::generate();
    let now = SystemTime::now();
    let text = link_code(&key, "Beep", now).expect("a link code");
    let uri = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("Link: "))
        .expect("the link line");
    let offer = LinkOffer::parse(uri).expect("an offer");
    assert_eq!(offer.server(now), Some(key.public()));
    assert!(text.contains(&offer.code()));
}
