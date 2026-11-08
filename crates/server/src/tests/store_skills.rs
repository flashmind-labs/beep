use super::*;
use crate::store::{TestDb, test_chat};

const PADEL: NewSkill = NewSkill {
    name: "Book padel",
    description: "Booking a padel court",
    body: "Ask for 19:00 first.",
};

fn patch_body(body: &str) -> SkillPatch<'_> {
    SkillPatch {
        body: Some(body),
        ..SkillPatch::default()
    }
}

#[tokio::test]
async fn group_skills_reach_members_only() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let (alice, bob) = (UserKey::for_test("alice"), UserKey::for_test("bob"));
    let flat = test_chat("flat");
    assert!(
        store
            .create_group(&alice, flat, "Flat", "Ali")
            .await
            .unwrap()
    );
    let owner = SkillOwner::Group(flat);
    let id = store.create_skill(&owner, PADEL).await.unwrap().unwrap();

    let seen = store.visible_skills(&alice).await.unwrap();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].group_name.as_deref(), Some("Flat"));
    assert!(store.visible_skills(&bob).await.unwrap().is_empty());
    assert!(!store.delete_skill(&bob, id).await.unwrap());
    assert_eq!(
        store.edit_skill(&bob, id, patch_body("x")).await.unwrap(),
        None
    );
    let own = SkillOwner::User(alice.clone());
    assert!(store.owned_skills(&own).await.unwrap().is_empty());
    assert_eq!(
        store
            .learn_skill_change(&own, id, patch_body("x"))
            .await
            .unwrap(),
        None
    );

    let used = store
        .use_skill(&owner, "Book padel")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(used.uses, 1);
    assert!(store.delete_skill(&alice, id).await.unwrap());
}
