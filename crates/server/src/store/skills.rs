//! Skills Beep learned with earlier versions, and each chat's skill review progress.

use super::Store;
use crate::account::user::UserKey;
use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::{PgConnection, Row, postgres::PgRow};
use uuid::Uuid;

/// The most skills one user or group keeps.
pub const SKILL_LIMIT: i64 = 100;

/// Versions kept per skill for undo.
const VERSIONS_KEPT: i64 = 10;

/// Whose skills: a user's own, or a group's.
#[derive(Clone)]
pub enum SkillOwner {
    User(UserKey),
    Group(Uuid),
}

impl SkillOwner {
    fn user_id(&self) -> Option<&str> {
        match self {
            Self::User(key) => Some(key.as_str()),
            Self::Group(_) => None,
        }
    }

    fn group_id(&self) -> Option<Uuid> {
        match self {
            Self::User(_) => None,
            Self::Group(id) => Some(*id),
        }
    }
}

/// A skill as the store keeps it.
#[derive(Debug, Clone, PartialEq)]
pub struct SkillRow {
    pub id: Uuid,
    pub group_id: Option<Uuid>,
    /// The group's name, for a group's skill.
    pub group_name: Option<String>,
    pub name: String,
    pub description: String,
    pub body: String,
    pub enabled: bool,
    pub uses: i32,
    pub last_used: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
    pub edited: bool,
    /// Whether an earlier version is kept.
    pub can_undo: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct NewSkill<'a> {
    pub name: &'a str,
    pub description: &'a str,
    pub body: &'a str,
}

/// What to change in a skill; `None` keeps it.
#[derive(Debug, Clone, Copy, Default)]
pub struct SkillPatch<'a> {
    pub name: Option<&'a str>,
    pub description: Option<&'a str>,
    pub body: Option<&'a str>,
    pub enabled: Option<bool>,
}

impl SkillPatch<'_> {
    fn changes_text(&self) -> bool {
        self.name.is_some() || self.description.is_some() || self.body.is_some()
    }
}

/// A chat's place in its skill reviews after counting a turn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Review {
    /// Turns since the last review, this one included.
    pub turns: i32,
    /// When the last review was; `None` before the first.
    pub since: Option<DateTime<Utc>>,
}

const COLUMNS: &str = "s.id, s.group_id, c.title, s.name, s.description, s.body, s.enabled, \
    s.uses, s.last_used, s.updated_at, s.edited, \
    EXISTS (SELECT 1 FROM skill_versions v WHERE v.skill_id = s.id)";

const FROM: &str = "skills s LEFT JOIN chats c ON c.id = s.group_id";

/// Skills the user may see and change: their own and their groups'.
const VISIBLE: &str = "(s.user_id = $1 OR s.group_id IN \
    (SELECT m.chat_id FROM chat_members m JOIN chats g ON g.id = m.chat_id \
     WHERE m.user_id = $1 AND g.kind = 'group'))";

fn skill_from(row: &PgRow) -> Result<SkillRow> {
    Ok(SkillRow {
        id: row.try_get(0)?,
        group_id: row.try_get(1)?,
        group_name: row.try_get(2)?,
        name: row.try_get(3)?,
        description: row.try_get(4)?,
        body: row.try_get(5)?,
        enabled: row.try_get(6)?,
        uses: row.try_get(7)?,
        last_used: row.try_get(8)?,
        updated_at: row.try_get(9)?,
        edited: row.try_get(10)?,
        can_undo: row.try_get(11)?,
    })
}

/// Keeps what skill `id` says now as its newest earlier version.
async fn save_version(tx: &mut PgConnection, id: Uuid) -> Result<()> {
    sqlx::query(
        "INSERT INTO skill_versions (skill_id, name, description, body, edited)
         SELECT id, name, description, body, edited FROM skills WHERE id = $1",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "DELETE FROM skill_versions WHERE skill_id = $1 AND seq NOT IN
         (SELECT seq FROM skill_versions WHERE skill_id = $1 ORDER BY seq DESC LIMIT $2)",
    )
    .bind(id)
    .bind(VERSIONS_KEPT)
    .execute(&mut *tx)
    .await?;
    Ok(())
}

async fn skill_by_id(tx: &mut PgConnection, id: Uuid) -> Result<Option<SkillRow>> {
    sqlx::query(&format!("SELECT {COLUMNS} FROM {FROM} WHERE s.id = $1"))
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .as_ref()
        .map(skill_from)
        .transpose()
}

/// Applies `patch` to skill `id`, saving the old text when it changes; `edited` marks a user change.
async fn patch(
    tx: &mut PgConnection,
    id: Uuid,
    patch: SkillPatch<'_>,
    edited: bool,
) -> Result<Option<SkillRow>> {
    if patch.changes_text() {
        save_version(tx, id).await?;
    }
    let done = sqlx::query(
        "UPDATE skills SET
            name = COALESCE($2, name),
            description = COALESCE($3, description),
            body = COALESCE($4, body),
            enabled = COALESCE($5, enabled),
            edited = CASE WHEN $6 THEN $7 ELSE edited END,
            updated_at = CASE WHEN $6 THEN now() ELSE updated_at END
         WHERE id = $1",
    )
    .bind(id)
    .bind(patch.name)
    .bind(patch.description)
    .bind(patch.body)
    .bind(patch.enabled)
    .bind(patch.changes_text())
    .bind(edited)
    .execute(&mut *tx)
    .await?;
    if done.rows_affected() == 0 {
        return Ok(None);
    }
    skill_by_id(tx, id).await
}

impl Store {
    /// The user's own skills then their groups', each most recently used first.
    pub(crate) async fn visible_skills(&self, user: &UserKey) -> Result<Vec<SkillRow>> {
        sqlx::query(&format!(
            "SELECT {COLUMNS} FROM {FROM} WHERE {VISIBLE}
             ORDER BY s.group_id NULLS FIRST, c.title, s.last_used DESC NULLS LAST, s.updated_at DESC"
        ))
        .bind(user.as_str())
        .fetch_all(&self.pool)
        .await?
        .iter()
        .map(skill_from)
        .collect()
    }

    /// The owner's skills, most recently used first.
    pub(crate) async fn owned_skills(&self, owner: &SkillOwner) -> Result<Vec<SkillRow>> {
        sqlx::query(&format!(
            "SELECT {COLUMNS} FROM {FROM}
             WHERE s.user_id IS NOT DISTINCT FROM $1 AND s.group_id IS NOT DISTINCT FROM $2
             ORDER BY s.last_used DESC NULLS LAST, s.updated_at DESC"
        ))
        .bind(owner.user_id())
        .bind(owner.group_id())
        .fetch_all(&self.pool)
        .await?
        .iter()
        .map(skill_from)
        .collect()
    }

    /// Adds a learned skill; `None` when the name is taken or at `SKILL_LIMIT`.
    pub async fn create_skill(
        &self,
        owner: &SkillOwner,
        skill: NewSkill<'_>,
    ) -> Result<Option<Uuid>> {
        let mut tx = self.pool.begin().await?;
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM skills
             WHERE user_id IS NOT DISTINCT FROM $1 AND group_id IS NOT DISTINCT FROM $2",
        )
        .bind(owner.user_id())
        .bind(owner.group_id())
        .fetch_one(&mut *tx)
        .await?;
        if count >= SKILL_LIMIT {
            return Ok(None);
        }
        let id = Uuid::new_v4();
        let added = sqlx::query(
            "INSERT INTO skills (id, user_id, group_id, name, description, body)
             VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT DO NOTHING",
        )
        .bind(id)
        .bind(owner.user_id())
        .bind(owner.group_id())
        .bind(skill.name)
        .bind(skill.description)
        .bind(skill.body)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok((added.rows_affected() == 1).then_some(id))
    }

    /// Changes one of the owner's skills on Beep's behalf.
    pub(crate) async fn learn_skill_change(
        &self,
        owner: &SkillOwner,
        id: Uuid,
        change: SkillPatch<'_>,
    ) -> Result<Option<SkillRow>> {
        let mut tx = self.pool.begin().await?;
        let owned: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM skills WHERE id = $1
             AND user_id IS NOT DISTINCT FROM $2 AND group_id IS NOT DISTINCT FROM $3)",
        )
        .bind(id)
        .bind(owner.user_id())
        .bind(owner.group_id())
        .fetch_one(&mut *tx)
        .await?;
        if !owned {
            return Ok(None);
        }
        let changed = patch(&mut tx, id, change, false).await?;
        tx.commit().await?;
        Ok(changed)
    }

    /// Edits a skill visible to the user; `None` when missing.
    pub(crate) async fn edit_skill(
        &self,
        user: &UserKey,
        id: Uuid,
        change: SkillPatch<'_>,
    ) -> Result<Option<SkillRow>> {
        let mut tx = self.pool.begin().await?;
        let visible: bool = sqlx::query_scalar(&format!(
            "SELECT EXISTS (SELECT 1 FROM skills s WHERE s.id = $2 AND {VISIBLE})"
        ))
        .bind(user.as_str())
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        if !visible {
            return Ok(None);
        }
        let changed = patch(&mut tx, id, change, true).await?;
        tx.commit().await?;
        Ok(changed)
    }

    pub(crate) async fn delete_skill(&self, user: &UserKey, id: Uuid) -> Result<bool> {
        let done = sqlx::query(&format!(
            "DELETE FROM skills s WHERE s.id = $2 AND {VISIBLE}"
        ))
        .bind(user.as_str())
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(done.rows_affected() == 1)
    }

    /// Restores a visible skill's newest earlier version; `None` when there is none.
    pub(crate) async fn undo_skill(&self, user: &UserKey, id: Uuid) -> Result<Option<SkillRow>> {
        let mut tx = self.pool.begin().await?;
        let restored = sqlx::query(&format!(
            "WITH last AS (
                DELETE FROM skill_versions WHERE skill_id = $2 AND seq =
                    (SELECT MAX(seq) FROM skill_versions WHERE skill_id = $2)
                RETURNING name, description, body, edited)
             UPDATE skills s SET name = last.name, description = last.description,
                body = last.body, edited = last.edited, updated_at = now()
             FROM last WHERE s.id = $2 AND {VISIBLE}"
        ))
        .bind(user.as_str())
        .bind(id)
        .execute(&mut *tx)
        .await?;
        if restored.rows_affected() == 0 {
            return Ok(None);
        }
        let skill = skill_by_id(&mut tx, id).await?;
        tx.commit().await?;
        Ok(skill)
    }

    /// The owner's enabled skill `name` (case-insensitive), counted as used.
    pub(crate) async fn use_skill(
        &self,
        owner: &SkillOwner,
        name: &str,
    ) -> Result<Option<SkillRow>> {
        let id: Option<Uuid> = sqlx::query_scalar(
            "UPDATE skills SET uses = uses + 1, last_used = now()
             WHERE user_id IS NOT DISTINCT FROM $1 AND group_id IS NOT DISTINCT FROM $2
             AND lower(name) = lower($3) AND enabled RETURNING id",
        )
        .bind(owner.user_id())
        .bind(owner.group_id())
        .bind(name.trim())
        .fetch_optional(&self.pool)
        .await?;
        let Some(id) = id else {
            return Ok(None);
        };
        let mut conn = self.pool.acquire().await?;
        skill_by_id(&mut conn, id).await
    }

    /// Counts a turn in `chat` towards its next skill review.
    pub(crate) async fn count_skill_turn(&self, chat: Uuid) -> Result<Review> {
        let row = sqlx::query(
            "INSERT INTO skill_reviews (chat_id, turns) VALUES ($1, 1)
             ON CONFLICT (chat_id) DO UPDATE SET turns = skill_reviews.turns + 1
             RETURNING turns, since",
        )
        .bind(chat)
        .fetch_one(&self.pool)
        .await?;
        Ok(Review {
            turns: row.try_get(0)?,
            since: row.try_get(1)?,
        })
    }

    /// Starts `chat`'s count again from `at`, when a review read up to.
    pub(crate) async fn reviewed_skills(&self, chat: Uuid, at: DateTime<Utc>) -> Result<()> {
        sqlx::query("UPDATE skill_reviews SET turns = 0, since = $2 WHERE chat_id = $1")
            .bind(chat)
            .bind(at)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
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
}
