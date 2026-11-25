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

#[test]
fn a_failed_check_waits_before_it_runs_again_whatever_changes() {
    let now = Instant::now();
    let mut retry = Retry::default();

    assert_eq!(retry.left(now), None);

    retry.failed(now);
    assert_eq!(
        retry.left(now + Duration::from_secs(60)),
        Some(RETRY_AFTER - Duration::from_secs(60))
    );
    assert_eq!(retry.left(now + RETRY_AFTER), None);
    assert_eq!(retry.left(now + RETRY_AFTER + Duration::from_secs(1)), None);
}
