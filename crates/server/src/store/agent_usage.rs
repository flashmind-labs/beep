//! Agent session spend per session and local day, and each agent's remaining plan, as nodes report them.

use super::Store;
use crate::{
    account::user::UserKey,
    protocol::{AgentSlug, ModelId, node::usage::LimitWindow},
};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use iroh::EndpointId;
use sqlx::{Row, postgres::PgRow, types::Json};

/// What one folder's sessions on one machine spent with one agent, today or earlier this week.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SpendRow {
    pub(crate) endpoint: EndpointId,
    pub(crate) cwd: String,
    pub(crate) agent: AgentSlug,
    /// Today rather than earlier this week.
    pub(crate) today: bool,
    /// `None` when none of its turns reported a cost.
    pub(crate) cost_usd: Option<f64>,
    pub(crate) tokens: u64,
    pub(crate) unpriced_tokens: u64,
}

/// The tokens one model of one agent used today.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ModelRow {
    pub(crate) agent: AgentSlug,
    pub(crate) model: ModelId,
    pub(crate) tokens: u64,
}

/// One agent's plan usage as the store keeps it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PlanRow {
    pub(crate) agent: AgentSlug,
    pub(crate) endpoint: EndpointId,
    /// Free-form text the agent reports, such as `max`.
    pub(crate) plan: Option<String>,
    pub(crate) windows: Vec<LimitWindow>,
    pub(crate) read_at: DateTime<Utc>,
}

/// The user's day: `$1` is the user's id.
const TODAY: &str =
    "(now() AT TIME ZONE COALESCE((SELECT timezone FROM users WHERE id=$1), 'UTC'))::date";

/// The node in `row`'s `endpoint_id`, which holds an `EndpointId`'s text.
pub(super) fn endpoint_of(row: &PgRow) -> Result<EndpointId> {
    let text: String = row.try_get("endpoint_id")?;
    text.parse()
        .with_context(|| format!("endpoint_id {text:?} is no node key"))
}

/// The agent in `row`'s `agent` column.
pub(super) fn agent_of(row: &PgRow) -> Result<AgentSlug> {
    Ok(AgentSlug::new(row.try_get::<String, _>("agent")?))
}

impl Store {
    /// This week's spend from Monday by machine, folder and agent, today apart.
    pub(crate) async fn week_spend(&self, user: &UserKey) -> Result<Vec<SpendRow>> {
        let rows = sqlx::query(&format!(
            "WITH t AS (SELECT {TODAY} AS today)
             SELECT endpoint_id, cwd, agent, day = t.today AS today, SUM(cost_usd) AS cost,
                 SUM(tokens)::BIGINT AS tokens, SUM(unpriced_tokens)::BIGINT AS unpriced
             FROM agent_usage, t
             WHERE user_id=$1 AND day >= date_trunc('week', t.today)::date AND day <= t.today
             GROUP BY endpoint_id, cwd, agent, day = t.today"
        ))
        .bind(user.as_ref())
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|row| {
                Ok(SpendRow {
                    endpoint: endpoint_of(row)?,
                    cwd: row.try_get("cwd")?,
                    agent: agent_of(row)?,
                    today: row.try_get("today")?,
                    cost_usd: row.try_get("cost")?,
                    tokens: u64::try_from(row.try_get::<i64, _>("tokens")?)?,
                    unpriced_tokens: u64::try_from(row.try_get::<i64, _>("unpriced")?)?,
                })
            })
            .collect()
    }

    /// The tokens each model used today, by agent.
    pub(crate) async fn today_models(&self, user: &UserKey) -> Result<Vec<ModelRow>> {
        let rows = sqlx::query(&format!(
            "SELECT agent, model, tokens FROM agent_model_usage
             WHERE user_id=$1 AND day = {TODAY} ORDER BY tokens DESC"
        ))
        .bind(user.as_ref())
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|row| {
                Ok(ModelRow {
                    agent: agent_of(row)?,
                    model: ModelId::new(row.try_get::<String, _>("model")?),
                    tokens: u64::try_from(row.try_get::<i64, _>("tokens")?)?,
                })
            })
            .collect()
    }

    /// Each agent's plan usage, by agent.
    pub(crate) async fn plan_usages(&self, user: &UserKey) -> Result<Vec<PlanRow>> {
        let rows = sqlx::query(
            "SELECT agent, endpoint_id, plan, windows, read_at FROM plan_usage
             WHERE user_id=$1 ORDER BY agent",
        )
        .bind(user.as_ref())
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|row| {
                let Json(windows): Json<Vec<LimitWindow>> = row.try_get("windows")?;
                Ok(PlanRow {
                    agent: agent_of(row)?,
                    endpoint: endpoint_of(row)?,
                    plan: row.try_get("plan")?,
                    windows,
                    read_at: row.try_get("read_at")?,
                })
            })
            .collect()
    }
}
