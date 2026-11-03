//! `skill`: loads a listed skill, and in the user's own chats creates and updates them.

use super::memory::{arg, opt_arg, reply};
use crate::{
    skills,
    store::{NewSkill, SkillOwner, SkillPatch, Store},
};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use serde_json::{Value, json};

pub(crate) const NAME: &str = "skill";

/// What a `skill` call does, by its `action`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::EnumString, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
enum Action {
    View,
    Create,
    Update,
}

impl Action {
    fn as_str(self) -> &'static str {
        self.into()
    }
}

pub(crate) struct SkillTool {
    pub store: Store,
    pub owner: SkillOwner,
    /// Whether it may create and update skills; a group's run only views.
    pub writes: bool,
}

impl SkillTool {
    fn actions(&self) -> &'static [Action] {
        if self.writes {
            &[Action::View, Action::Create, Action::Update]
        } else {
            &[Action::View]
        }
    }
}

#[async_trait]
impl Tool for SkillTool {
    fn name(&self) -> &str {
        NAME
    }
    fn description(&self) -> &str {
        if self.writes {
            "Skills: how the user likes a recurring task done. `view` loads one the context lists, by \
             name: the steps and preferences to follow; load it before starting the task it covers. \
             `create` saves a new one when the user asks to keep a way of doing something: a short \
             name, a one-line description of when it applies, and the steps as the body. `update` \
             rewrites an existing one by name with the full new body."
        } else {
            "Load one of the skills the context lists, by its name: the steps and preferences to \
             follow for that task. Load it before starting the task it covers."
        }
    }
    fn parameters(&self) -> Value {
        let actions: Vec<&str> = self.actions().iter().map(|a| a.as_str()).collect();
        let mut properties = json!({
            "action": {"type": "string", "enum": actions},
            "name": {"type": "string", "description": "The skill's name as listed, or a new one's."}
        });
        if self.writes
            && let Some(properties) = properties.as_object_mut()
        {
            properties.insert(
                "description".into(),
                json!({"type": "string", "description": format!("create, update: when it applies, at most {} characters.", skills::DESCRIPTION_MAX)}),
            );
            properties.insert(
                "body".into(),
                json!({"type": "string", "description": "create, update: the steps and preferences, in Markdown."}),
            );
        }
        json!({"type": "object", "properties": properties, "required": ["action", "name"], "additionalProperties": false})
    }
    fn humanize(&self, args: &Value) -> String {
        match action(args) {
            Ok(Action::Create) => "Saving a skill",
            Ok(Action::Update) => "Updating a skill",
            _ => "Loading a skill",
        }
        .to_owned()
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        let action = match action(&ctx.args) {
            Ok(action) if self.actions().contains(&action) => action,
            Ok(action) => {
                return Ok(ToolResult::failure(
                    ctx.tool_call_id,
                    format!("skill {} is not available here.", action.as_str()),
                ));
            }
            Err(error) => return Ok(ToolResult::failure(ctx.tool_call_id, error.to_string())),
        };
        let name = arg(&ctx.args, "name")?.trim();
        match action {
            Action::View => self.view(&ctx, name).await,
            Action::Create => self.create(&ctx, name).await,
            Action::Update => self.update(&ctx, name).await,
        }
    }
}

/// The `action` of a call; a call without one views, as `skill_view` did.
fn action(args: &Value) -> Result<Action> {
    let Some(name) = opt_arg(args, "action") else {
        return Ok(Action::View);
    };
    name.parse()
        .map_err(|_| anyhow!("unknown action {name:?}; use view, create or update"))
}

impl SkillTool {
    async fn view(&self, ctx: &ToolContext<'_>, name: &str) -> Result<ToolResult> {
        match self.store.use_skill(&self.owner, name).await? {
            Some(skill) => reply(
                ctx,
                json!({"name": skill.name, "description": skill.description, "body": skill.body}),
            ),
            None => Ok(ToolResult::failure(
                ctx.tool_call_id,
                format!("No skill called \"{name}\"."),
            )),
        }
    }

    async fn create(&self, ctx: &ToolContext<'_>, name: &str) -> Result<ToolResult> {
        let description = opt_arg(&ctx.args, "description").unwrap_or_default().trim();
        let body = opt_arg(&ctx.args, "body").unwrap_or_default().trim();
        if !skills::valid(name, description, body) {
            return Ok(ToolResult::failure(ctx.tool_call_id, limits()));
        }
        let skill = NewSkill {
            name,
            description,
            body,
        };
        match self.store.create_skill(&self.owner, skill).await? {
            Some(id) => reply(ctx, json!({"created": true, "id": id, "name": name})),
            None => Ok(ToolResult::failure(
                ctx.tool_call_id,
                format!(
                    "Not saved: a skill called \"{name}\" exists (update it instead), or the user \
                     has {} skills already.",
                    crate::store::SKILL_LIMIT
                ),
            )),
        }
    }

    async fn update(&self, ctx: &ToolContext<'_>, name: &str) -> Result<ToolResult> {
        let skills = self.store.owned_skills(&self.owner).await?;
        let Some(skill) = skills
            .iter()
            .find(|skill| skill.name.eq_ignore_ascii_case(name))
        else {
            return Ok(ToolResult::failure(
                ctx.tool_call_id,
                format!("No skill called \"{name}\"."),
            ));
        };
        let description = opt_arg(&ctx.args, "description").map(str::trim);
        let body = opt_arg(&ctx.args, "body").map(str::trim);
        if !skills::valid(
            &skill.name,
            description.unwrap_or(&skill.description),
            body.unwrap_or(&skill.body),
        ) {
            return Ok(ToolResult::failure(ctx.tool_call_id, limits()));
        }
        let patch = SkillPatch {
            description,
            body,
            ..SkillPatch::default()
        };
        match self
            .store
            .learn_skill_change(&self.owner, skill.id, patch)
            .await?
        {
            Some(changed) => reply(ctx, json!({"updated": true, "name": changed.name})),
            None => Ok(ToolResult::failure(
                ctx.tool_call_id,
                format!("No skill called \"{name}\"."),
            )),
        }
    }
}

fn limits() -> String {
    format!(
        "Not saved: a skill needs a name of at most {} characters, a description of at most {} \
         and a body of at most {} bytes, none of them empty.",
        skills::NAME_MAX,
        skills::DESCRIPTION_MAX,
        skills::BODY_MAX
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{store::TestDb, user::UserKey};
    use flashmind::core::CancellationToken;

    async fn call(tool: &SkillTool, args: Value) -> ToolResult {
        let cancel = CancellationToken::new();
        tool.execute(ToolContext::new("call", args, None, &cancel))
            .await
            .expect("the store answers")
    }

    #[tokio::test]
    async fn a_skill_saved_in_the_chat_can_be_viewed_and_rewritten_at_once() {
        let db = TestDb::new().await.expect("test database");
        let store = db.store().await.expect("store");
        let tool = SkillTool {
            store,
            owner: SkillOwner::User(UserKey::for_test("alice")),
            writes: true,
        };
        let created = call(
            &tool,
            json!({"action": "create", "name": "Book padel", "description": "Booking a court", "body": "Ask for 19:00."}),
        )
        .await;
        assert!(created.is_success(), "{}", created.output());

        let viewed = call(&tool, json!({"action": "view", "name": "book padel"})).await;
        assert!(viewed.output().contains("Ask for 19:00."));

        let updated = call(
            &tool,
            json!({"action": "update", "name": "Book padel", "body": "Ask for 20:00."}),
        )
        .await;
        assert!(updated.is_success(), "{}", updated.output());
        let viewed = call(&tool, json!({"action": "view", "name": "Book padel"})).await;
        assert!(viewed.output().contains("Ask for 20:00."));
        assert!(viewed.output().contains("Booking a court"));
    }

    #[tokio::test]
    async fn a_group_only_views_skills() {
        let db = TestDb::new().await.expect("test database");
        let store = db.store().await.expect("store");
        let tool = SkillTool {
            store,
            owner: SkillOwner::User(UserKey::for_test("alice")),
            writes: false,
        };
        assert_eq!(
            tool.parameters()["properties"]["action"]["enum"],
            json!(["view"])
        );
        let refused = call(
            &tool,
            json!({"action": "create", "name": "x", "description": "y", "body": "z"}),
        )
        .await;
        assert!(!refused.is_success());
    }
}
