use super::*;
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
    let offers = Offers::default();
    offers.agents.share_folders(vec![shared.path().to_owned()]);
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
