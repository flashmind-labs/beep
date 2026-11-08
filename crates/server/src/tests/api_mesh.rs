use super::*;
use crate::{
    protocol::DeviceKind,
    store::{NewKey, TestDb},
};
use iroh::SecretKey;

#[tokio::test]
async fn a_new_url_clears_the_stored_fork() {
    let db = TestDb::new().await.unwrap();
    let user = User::for_test(db.store().await.unwrap(), "alice");
    let key = SecretKey::generate().public();
    user.link_key(NewKey {
        endpoint: key,
        name: "desk",
        kind: DeviceKind::Desktop {},
    })
    .await
    .unwrap();
    let machine = key.to_string();
    let request = |url: &str| MirrorImport {
        project: "org/widget".into(),
        machine_id: machine.clone(),
        folder: "/srv/widget".into(),
        url: url.into(),
    };
    record(&user, &request("https://example.com/org/widget"))
        .await
        .unwrap();
    let fork = "me/widget".parse::<RepoName>().unwrap();
    user.set_import_fork("org/widget", &machine, Some(&fork))
        .await
        .unwrap();

    record(&user, &request("https://example.com/org/widget"))
        .await
        .unwrap();
    assert_eq!(user.imports().await.unwrap()[0].fork, Some(fork));

    record(&user, &request("https://example.org/org/widget"))
        .await
        .unwrap();
    assert_eq!(user.imports().await.unwrap()[0].fork, None);
}
