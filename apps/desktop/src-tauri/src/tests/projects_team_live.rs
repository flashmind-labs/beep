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
            team: true,
            windows: BTreeSet::from(["main".to_owned()]),
            acting: None,
            refused: BTreeSet::new(),
            asked: None,
            sent: None,
            asking: false,
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

#[test]
fn a_project_with_no_team_follows_no_cards() {
    let mut held = Held::default();
    let id = followed(&mut held, "org/web");
    held.projects.get_mut(&id).unwrap().team = false;

    assert!(held.interests(machine()).is_empty());
}

#[test]
fn a_page_that_leaves_ends_the_follow_of_its_project_only() {
    let mut held = Held::default();
    let (web, api) = (
        followed(&mut held, "org/web"),
        followed(&mut held, "org/api"),
    );
    held.projects
        .get_mut(&api)
        .unwrap()
        .windows
        .insert("other".to_owned());

    held.hide("main", Some(web));
    assert!(!held.projects.contains_key(&web));
    assert!(held.projects.contains_key(&api));

    held.hide("main", None);
    assert!(held.projects.contains_key(&api));
    held.hide("other", Some(api));
    assert!(held.interests(machine()).is_empty());
}

#[test]
fn an_old_node_is_asked_for_the_cards_while_the_page_shows_them() {
    let mut held = Held::default();
    let id = followed(&mut held, "org/web");
    let old = machine();

    // Nothing asks while every node answers the topic.
    assert!(!held.start_asking(id));

    held.cards_heard("org/web", old, Heard::Refused);
    assert!(held.start_asking(id));
    // One task asks for it.
    assert!(!held.start_asking(id));
    assert!(held.keep_asking(id));

    // The node answering the topic ends it, and so does the page leaving.
    held.cards_heard("org/web", old, Heard::Missing);
    assert!(!held.keep_asking(id));
    held.cards_heard("org/web", old, Heard::Refused);
    assert!(held.start_asking(id));
    held.hide("main", Some(id));
    assert!(!held.keep_asking(id));
}

#[test]
fn what_an_old_node_gave_goes_when_it_answers_the_topic() {
    let mut held = Held::default();
    let id = followed(&mut held, "org/web");
    let old = machine();

    held.cards_heard("org/web", old, Heard::Refused);
    held.projects.get_mut(&id).unwrap().asked = Some(TeamCards::default());
    assert!(held.projects[&id].cards().is_some());

    held.cards_heard("org/web", old, Heard::Missing);
    assert!(held.projects[&id].cards().is_none());
}
