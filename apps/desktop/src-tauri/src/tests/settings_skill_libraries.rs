use super::*;

fn only(names: &[&str]) -> Pick {
    Pick::Only(names.iter().map(|name| (*name).to_owned()).collect())
}

#[test]
fn uninstalling_a_skill_from_all_keeps_the_rest() {
    let all = ["crawl", "map", "scrape"];

    assert_eq!(
        without_skill(&Pick::All, "map", &all),
        only(&["crawl", "scrape"])
    );
    assert_eq!(without_skill(&only(&["map"]), "map", &all), Pick::None);
    assert_eq!(with_skill(&Pick::None, "map"), only(&["map"]));
    assert_eq!(with_skill(&Pick::All, "map"), Pick::All);
}

#[test]
fn repository_urls_compare_loosely() {
    assert_eq!(
        same_repo("https://host/Org/Repo.git/"),
        same_repo("https://host/org/repo")
    );
}
