use super::*;
use crate::{
    account::user::User,
    store::{Blobs, NewMessage, PgBlobs, TestDb, TurnsToDelete, test_chat, test_turn},
};
use async_trait::async_trait;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

fn picture(data: &[u8]) -> NewPicture<'_> {
    NewPicture {
        mime: "image/jpeg",
        data,
        width: 640,
        height: 480,
    }
}

#[tokio::test]
async fn a_file_reads_back_in_parts_for_its_owner_only() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let alice = User::for_test(store.clone(), "alice");
    let bob = User::for_test(store.clone(), "bob");
    let data: Vec<u8> = (0..=255).collect();
    let s1 = test_chat("s1");
    test_turn(&store, &UserKey::for_test("alice"), s1, "hi", 1000)
        .await
        .unwrap();
    let id = alice
        .save_file(s1, "pass.pdf", "application/pdf", &data)
        .await
        .unwrap();
    let first = alice.file_part(&id, 0, 100).await.unwrap().unwrap();
    assert_eq!(first.size, 256);
    assert_eq!(first.data, data[..100]);
    let last = alice.file_part(&id, 200, 100).await.unwrap().unwrap();
    assert_eq!(last.data, data[200..]);
    let past = alice.file_part(&id, 300, 100).await.unwrap().unwrap();
    assert!(past.data.is_empty());
    for offset in [u64::from(u32::MAX), u64::MAX] {
        let far = alice.file_part(&id, offset, 100).await.unwrap().unwrap();
        assert!(far.data.is_empty());
        assert_eq!(far.size, 256);
    }
    let rest = alice.file_part(&id, 200, u64::MAX).await.unwrap().unwrap();
    assert_eq!(rest.data, data[200..]);
    assert!(bob.file_part(&id, 0, 100).await.unwrap().is_none());

    alice.delete_session(s1).await.unwrap();
    assert!(alice.file_part(&id, 0, 100).await.unwrap().is_none());
}

/// Blobs in Postgres whose second write fails.
struct SecondPutFails {
    inner: PgBlobs,
    puts: AtomicUsize,
}

#[async_trait]
impl Blobs for SecondPutFails {
    async fn put(&self, id: &str, data: &[u8]) -> Result<()> {
        let put = self.puts.fetch_add(1, Ordering::Relaxed);
        if put == 1 {
            anyhow::bail!("the blob store is down");
        }
        self.inner.put(id, data).await
    }

    async fn part(&self, id: &str, offset: u64, len: u64) -> Result<Option<FilePart>> {
        self.inner.part(id, offset, len).await
    }

    async fn delete(&self, ids: &[String]) -> Result<()> {
        self.inner.delete(ids).await
    }
}

#[tokio::test]
async fn a_failed_save_keeps_none_of_the_messages_files() {
    let db = TestDb::new().await.unwrap();
    let mut store = db.store().await.unwrap();
    let alice = UserKey::for_test("alice");
    let s1 = test_chat("s1");
    let one = test_turn(&store, &alice, s1, "one", 1000).await.unwrap();
    let of = PicturesOf {
        chat: s1,
        message: one,
    };
    let pool = store.pool().clone();
    let fails = || {
        Arc::new(SecondPutFails {
            inner: PgBlobs::new(pool.clone()),
            puts: AtomicUsize::new(0),
        })
    };
    let rows = || async {
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM files")
            .fetch_one(&pool)
            .await
            .unwrap()
    };

    store.blobs = fails();
    store
        .save_pictures(&alice, of, &[picture(b"one"), picture(b"two")])
        .await
        .expect_err("the second picture fails");
    assert_eq!(rows().await, 0);
    assert!(store.session_pictures(&alice, s1).await.unwrap().is_empty());

    store.blobs = fails();
    let files = [
        NewAttachment {
            name: "a.pdf",
            mime: "application/pdf",
            data: b"a",
        },
        NewAttachment {
            name: "b.pdf",
            mime: "application/pdf",
            data: b"b",
        },
    ];
    store
        .save_attachments(&alice, of, &files)
        .await
        .expect_err("the second file fails");
    assert_eq!(rows().await, 0);
    assert!(store.session_files(&alice, s1).await.unwrap().is_empty());
}

#[tokio::test]
async fn session_pictures_load_by_turn_and_go_with_the_turn_or_chat() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let alice = UserKey::for_test("alice");
    let bob = UserKey::for_test("bob");
    let s1 = test_chat("s1");
    let one = test_turn(&store, &alice, s1, "one", 1000).await.unwrap();
    let two = test_turn(&store, &alice, s1, "two", 2000).await.unwrap();
    let of = |message| PicturesOf { chat: s1, message };
    let first = store
        .save_pictures(&alice, of(one), &[picture(b"one"), picture(b"two")])
        .await
        .unwrap();
    let second = store
        .save_pictures(&alice, of(two), &[picture(b"three")])
        .await
        .unwrap();
    assert_eq!(
        (first[0].width, first[0].height, first[0].mime.as_str()),
        (640, 480, "image/jpeg")
    );

    let listed = store.session_pictures(&alice, s1).await.unwrap();
    assert_eq!(
        listed,
        [
            SentImages {
                message: 1000,
                images: first.clone()
            },
            SentImages {
                message: 2000,
                images: second.clone()
            },
        ]
    );
    let read = store.file_part(&alice, &first[1].id, 0, 100).await.unwrap();
    assert_eq!(read.unwrap().data, b"two");
    assert!(store.session_pictures(&bob, s1).await.unwrap().is_empty());
    assert!(
        store
            .file_part(&bob, &first[0].id, 0, 100)
            .await
            .unwrap()
            .is_none()
    );

    // Pictures outlive the 30 days sent files get.
    sqlx::query("UPDATE files SET created_at = now() - interval '90 days'")
        .execute(store.pool())
        .await
        .unwrap();
    store
        .save_file(
            &alice,
            NewFile {
                chat: test_chat("s2"),
                name: "a.pdf",
                mime: "application/pdf",
                data: b"pdf",
            },
        )
        .await
        .unwrap();
    assert_eq!(store.session_pictures(&alice, s1).await.unwrap().len(), 2);

    // Deleting a turn takes its pictures; deleting the chat takes the rest.
    store
        .delete_turn(&alice, s1, 1000, TurnsToDelete::One)
        .await
        .unwrap();
    assert_eq!(
        store.session_pictures(&alice, s1).await.unwrap(),
        [SentImages {
            message: 2000,
            images: second.clone()
        }]
    );
    assert!(
        store
            .file_part(&alice, &first[0].id, 0, 100)
            .await
            .unwrap()
            .is_none()
    );
    store.delete_sessions(&alice, &[s1]).await.unwrap();
    assert!(store.session_pictures(&alice, s1).await.unwrap().is_empty());
    assert!(
        store
            .file_part(&alice, &second[0].id, 0, 100)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn group_pictures_are_every_members_until_the_group_goes() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let alice = UserKey::for_test("alice");
    let bob = UserKey::for_test("bob");
    let user = UserKey::for_test("user");
    let g1 = test_chat("g1");
    store
        .create_group(&alice, g1, "Trip", "Alice")
        .await
        .unwrap();
    let token = store.group_invite(&alice, g1, false).await.unwrap();
    store
        .join_group(&bob, &token.unwrap(), "Bob")
        .await
        .unwrap();
    let posted = store
        .post_to_group(
            &alice,
            NewMessage {
                group_id: g1,
                request_id: test_chat("r1"),
                content: "look",
            },
        )
        .await
        .unwrap()
        .unwrap();
    let of = PicturesOf {
        chat: g1,
        message: posted.message.id,
    };
    let saved = store
        .save_pictures(&alice, of, &[picture(b"view")])
        .await
        .unwrap();

    assert_eq!(
        store.group_pictures(g1).await.unwrap(),
        [SentImages {
            message: posted.message.id,
            images: saved.clone()
        }]
    );
    let read = store.file_part(&bob, &saved[0].id, 0, 100).await.unwrap();
    assert_eq!(read.unwrap().data, b"view");
    assert!(
        store
            .file_part(&user, &saved[0].id, 0, 100)
            .await
            .unwrap()
            .is_none()
    );

    // The sender leaving keeps them for the group; the last one out deletes them.
    store.leave_group(&alice, g1).await.unwrap();
    assert!(
        store
            .file_part(&bob, &saved[0].id, 0, 100)
            .await
            .unwrap()
            .is_some()
    );
    store.leave_group(&bob, g1).await.unwrap();
    assert!(store.group_pictures(g1).await.unwrap().is_empty());
}
