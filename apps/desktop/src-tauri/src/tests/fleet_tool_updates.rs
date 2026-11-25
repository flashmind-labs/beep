use super::*;

#[tokio::test]
async fn a_package_the_lookup_missed_is_not_looked_up_again_until_the_releases_are_old() {
    let looked = Instant::now();
    *AGENT_RELEASES.lock().unwrap() = Some((looked, HashMap::from([("no-such-package", None)])));

    let releases = agent_releases(&BTreeSet::from(["no-such-package"])).await;

    assert_eq!(releases.get("no-such-package"), Some(&None));
    let held = AGENT_RELEASES.lock().unwrap();
    assert_eq!(held.as_ref().map(|(at, _)| *at), Some(looked));
}
