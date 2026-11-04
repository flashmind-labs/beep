//! Memories as coding agents reach them through the Beep MCP server, and the standing
//! rules a session starts with. A session is one user's, on their machine, but what it
//! reads goes to the agent's model and may run commands, so mail and Slack memories stay
//! out, as they stay out of a group.

use super::{
    group_memory::shareable,
    memory::{ScopeNames, arg, fact, opt_arg},
};
use crate::{
    account::user::User,
    protocol::MemorySource,
    store::{MemoryRow, Origin, Scope},
};
use anyhow::{Result, bail, ensure};
use chrono::NaiveDate;
use serde_json::Value;

/// The longest memory an agent may save, as the Memories screen allows.
const MAX_CONTENT: usize = 4000;
/// Most memories one `recall` gives, and how many it looks through to find them.
const RECALL_LIMIT: usize = 30;
const RECALL_LOOKS: usize = 100;
/// The filing of what agents save, which makes it a standing rule at once.
const RULE_CATEGORY: &str = "preference";
/// Most standing rules a session starts with, in all and in bytes; `recall` finds the rest.
const MAX_RULES: usize = 12;
const MAX_RULES_BYTES: usize = 1500;
/// A longer memory is not short enough to go in every session's first prompt.
const MAX_RULE_BYTES: usize = 240;
const RULES_HEADER: &str =
    "Beep: the user's standing rules, from their memories (recall finds more):";

fn origin(session: &str) -> Origin {
    Origin {
        reference: session.to_owned(),
        quote: None,
    }
}

fn content(args: &Value) -> Result<&str> {
    let content = arg(args, "content")?;
    ensure!(
        !content.trim().is_empty() && content.len() <= MAX_CONTENT,
        "`content` must be 1 to {MAX_CONTENT} bytes"
    );
    Ok(content)
}

/// Saves what the user stated, as the chat's `memory` store does.
pub(crate) async fn remember(user: &User, session: &str, args: &Value) -> Result<String> {
    content(args)?;
    let names = ScopeNames::load(user).await?;
    let mut fact = fact(args, MemorySource::User {}, Some(origin(session)))?;
    fact.category = Some(RULE_CATEGORY);
    fact.scope = names.arg(args)?;
    let remembered = user.remember(fact).await?;
    let change = if remembered.replaced {
        "Already remembered"
    } else {
        "Remembered"
    };
    Ok(format!(
        "{change}; new sessions get it and the user sees it in Memories. Its id is {}.",
        remembered.id
    ))
}

pub(crate) async fn recall(user: &User, args: &Value) -> Result<String> {
    let names = ScopeNames::load(user).await?;
    let scope = names.arg(args)?;
    let query = opt_arg(args, "query").unwrap_or_default();
    let hits: Vec<Value> = user
        .search_memories(query, None, scope.as_ref(), RECALL_LOOKS)
        .await?
        .into_iter()
        .filter(|hit| shareable(hit.source))
        .take(RECALL_LIMIT)
        .map(|hit| hit.to_json(names.name(&hit.scope).as_deref()))
        .collect();
    if hits.is_empty() {
        return Ok("No memories found.".to_owned());
    }
    Ok(Value::Array(hits).to_string())
}

/// The memory `id`, when it is one an agent can see.
async fn visible(user: &User, id: &str) -> Result<MemoryRow> {
    let rows = user.memory_rows().await?;
    match rows
        .into_iter()
        .find(|row| row.id == id && shareable(row.source))
    {
        Some(row) => Ok(row),
        None => bail!("There is no memory with the id {id}."),
    }
}

pub(crate) async fn update(user: &User, session: &str, args: &Value) -> Result<String> {
    content(args)?;
    let id = arg(args, "id")?;
    let row = visible(user, id).await?;
    let names = ScopeNames::load(user).await?;
    let mut fact = fact(args, MemorySource::User {}, Some(origin(session)))?;
    // A corrected rule stays a rule rather than being filed anew.
    fact.category = Some(&row.category);
    fact.scope = names.arg(args)?;
    ensure!(
        user.update_memory(id, fact).await?,
        "There is no memory with the id {id}."
    );
    Ok("Updated.".to_owned())
}

pub(crate) async fn forget(user: &User, args: &Value) -> Result<String> {
    let id = arg(args, "id")?;
    visible(user, id).await?;
    ensure!(
        user.forget(id).await?,
        "There is no memory with the id {id}."
    );
    Ok("Forgotten.".to_owned())
}

/// The standing rules a session starts with; empty when there are none.
pub(crate) async fn standing_rules(user: &User) -> Result<String> {
    let rows = user.memory_rows().await?;
    let names = ScopeNames::load(user).await?;
    Ok(rules(&rows, user.today().await, &names))
}

/// What the user stated as a lasting preference and is short: the newest first, those
/// about the user ahead of those about a machine, project or account.
fn rules(rows: &[MemoryRow], today: NaiveDate, names: &ScopeNames) -> String {
    let mut standing: Vec<&MemoryRow> = rows
        .iter()
        .filter(|row| {
            row.source == (MemorySource::User {})
                && row.category == RULE_CATEGORY
                && row.valid.until.is_none()
                && row.valid.from.is_none_or(|from| from <= today)
                && row.content.len() <= MAX_RULE_BYTES
        })
        .collect();
    standing.sort_by_key(|row| row.scope != Scope::User);
    let mut text = String::from(RULES_HEADER);
    let mut count = 0;
    for row in standing {
        let line = match names.name(&row.scope) {
            Some(scope) => format!("\n- [{scope}] {}", row.content),
            None => format!("\n- {}", row.content),
        };
        // A rule too long for what is left gives way to shorter ones.
        if text.len() + line.len() > MAX_RULES_BYTES {
            continue;
        }
        text.push_str(&line);
        count += 1;
        if count == MAX_RULES {
            break;
        }
    }
    if count == 0 { String::new() } else { text }
}
