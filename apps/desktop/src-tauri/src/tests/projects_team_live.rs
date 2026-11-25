use super::*;

fn machine() -> EndpointId {
    iroh::SecretKey::generate().public()
}

fn followed(held: &mut Held, name: &str) -> Uuid {
    let id = Uuid::new_v4();
    held.projects.insert(
        id,
        Project {
            name: name.to_owned(),
            windows: BTreeSet::from(["main".to_owned()]),
            acting: None,
            refused: BTreeSet::new(),
            asked: None,
            sent: None,
        },
    );
    id
}

#[test]
fn the_cards_come_from_the_machine_that_acts_and_go_when_it_stops() {
    let mut held = Held::default();
    let id = followed(&mut held, "org/web");
    let (acting, other) = (machine(), machine());

    assert_eq!(held.cards_heard("org/web", other, Heard::Missing), vec![id]);
    assert!(held.projects[&id].cards().is_none());

    held.cards_heard("org/web", acting, Heard::Cards(TeamCards::default()));
    assert!(held.projects[&id].cards().is_some());

    // Another machine saying it has nothing leaves the acting machine's cards.
    held.cards_heard("org/web", other, Heard::Missing);
    assert!(held.projects[&id].cards().is_some());

    held.cards_heard("org/web", acting, Heard::Missing);
    assert!(held.projects[&id].cards().is_none());
}

#[test]
fn another_project_hears_nothing_of_these_cards() {
    let mut held = Held::default();
    followed(&mut held, "org/web");

    assert!(
        held.cards_heard("org/api", machine(), Heard::Missing)
            .is_empty()
    );
}

#[test]
fn a_closed_window_ends_its_follows_and_the_interests_follow_what_is_left() {
    let mut held = Held::default();
    let me = machine();
    followed(&mut held, "org/web");
    held.invites.insert("main".to_owned());
    assert_eq!(held.interests(me).len(), 2);

    held.forget("main");
    assert!(held.interests(me).is_empty());
}

#[test]
fn a_refusing_node_is_remembered_until_it_answers_the_topic() {
    let mut held = Held::default();
    let id = followed(&mut held, "org/web");
    let old = machine();

    held.cards_heard("org/web", old, Heard::Refused);
    assert!(held.projects[&id].refused.contains(&old));

    held.cards_heard("org/web", old, Heard::Missing);
    assert!(held.projects[&id].refused.is_empty());
}
