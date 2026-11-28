use super::*;
use crate::agents::Parts;
use crate::link::Offers;

// Two lookups of one source overlap: the one that started first finishes last
// and must not put its older answer over the newer one.
#[cfg(unix)]
#[tokio::test]
async fn an_older_lookup_that_lands_last_does_not_replace_a_newer_one() {
    let shared = tempfile::tempdir().unwrap();
    let old = shared.path().join("old");
    let new = shared.path().join("new");
    std::fs::create_dir(&old).unwrap();
    std::fs::create_dir(&new).unwrap();
    let link = shared.path().join("link");
    std::os::unix::fs::symlink(&old, &link).unwrap();
    let offers = Offers::from_parts(Parts {
        folders: vec![shared.path().to_owned()],
        ..Parts::default()
    });
    let topic = Topic::keyed("files", link.to_str().unwrap());
    let source = Source::new(&offers, &topic).await.unwrap();
    let before = crate::lock(&source.roots).clone();
    assert_eq!(before.values().next(), Some(&old.canonicalize().unwrap()));

    let (reached, reached_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release) = tokio::sync::oneshot::channel();
    *crate::lock(&source.after_lookup) = Some(Gate {
        root: PathBuf::new(),
        reached,
        release,
    });
    let source = Arc::new(source);
    let first = tokio::spawn({
        let (source, offers) = (source.clone(), offers.clone());
        async move { source.resolve(&offers).await }
    });
    // The first lookup has the old folder and is held before it stores.
    reached_rx.await.unwrap();

    std::fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink(&new, &link).unwrap();
    source.resolve(&offers).await.unwrap();
    release_tx.send(()).unwrap();
    first.await.unwrap().unwrap();

    let roots = crate::lock(&source.roots).clone();
    assert_eq!(roots.values().next(), Some(&new.canonicalize().unwrap()));
}

// A root without Git metadata asks Git again at most once per `GIT_RETRY`.
#[tokio::test(start_paused = true)]
async fn a_root_without_git_retries_once_per_retry_interval() {
    let root = tempfile::tempdir().unwrap();
    let offers = Offers::from_parts(Parts {
        folders: vec![root.path().to_owned()],
        ..Parts::default()
    });
    let topic = Topic::keyed("files", root.path().to_str().unwrap());
    let source = Source::new(&offers, &topic).await.unwrap();
    source.bind(&offers).await.unwrap();
    assert!(source.has_bare());

    let changed = [topic];
    assert!(source.retry_git(changed.iter()));
    assert!(!source.retry_git(changed.iter()));
    tokio::time::advance(GIT_RETRY).await;
    assert!(source.retry_git(changed.iter()));
    assert!(!source.retry_git(changed.iter()));
}

// The device list is read for the two topics that say it changed, while a
// stream holds their source, and for no other.
#[tokio::test]
async fn only_the_device_topics_follow_the_devices() {
    let offers = Offers::default();
    let followed = |name: &'static str| {
        let offers = offers.clone();
        async move {
            let source = Source::new(&offers, &Topic::new(name)).await.unwrap();
            source._devices.is_some()
        }
    };

    assert!(followed("device-list").await);
    assert!(followed("devices").await);
    assert!(!followed("*").await);
}
