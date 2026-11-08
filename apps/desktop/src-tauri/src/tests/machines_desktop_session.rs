use super::*;

/// Takes a real screenshot and tree on this Mac and sends both through
/// the wire encoding. Needs Screen Recording and Accessibility for the
/// process running it, so it only runs by hand:
/// `cargo test -- --ignored live_desktop --nocapture`.
#[test]
#[ignore = "needs a desktop session and macOS permissions"]
fn live_desktop() {
    let mut session = mac::Session::default();
    println!("{:?}", permissions());
    for request in [
        Request::Screenshot { max_side: 1280 },
        Request::Tree { max_elements: 60 },
    ] {
        let reply = session.handle(request);
        let body = rmp_serde::to_vec(&reply).expect("a reply encodes");
        let back: Reply = rmp_serde::from_slice(&body).expect("a reply decodes");
        match &back {
            Reply::Tree(tree) => println!("{} ({} bytes)\n{}", tree.app, body.len(), tree.text),
            other => println!("{other:?} ({} bytes)", body.len()),
        }
        assert_eq!(back, reply);
    }
}
