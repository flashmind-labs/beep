//! What coding agent sessions on the user's machines spent, per session
//! and day in the user's timezone, and how much of each agent's plan is
//! left, as nodes report them (see `crate::nodes::usage`).

use super::Store;
use crate::{
    protocol::{
        AgentSlug, ModelId,
        node::usage::{LimitWindow, MAX_WINDOW_MINUTES, PlanUsage, TurnUsage},
    },
    user::UserKey,
};
use anyhow::{Context, Result, bail};
use chrono::{DateTime, NaiveDate, Utc};
use iroh::EndpointId;
use sqlx::{Row, postgres::PgRow, types::Json};

/// The longest agent, model or session name kept, and the longest folder.
const MAX_NAME: usize = 200;
const MAX_FOLDER: usize = 4096;
/// The most one turn may cost; more is a report gone wrong.
const MAX_TURN_COST: f64 = 10_000.0;
/// The most windows one plan keeps.
const MAX_WINDOWS: usize = 16;

/// What the sessions in one folder of one machine spent with one agent,
/// today or earlier this week.
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

fn name(what: &str, value: &str, max: usize) -> Result<()> {
    if value.is_empty() || value.len() > max {
        bail!("the {what} is empty or too long");
    }
    Ok(())
}

/// The node in `row`'s `endpoint_id`, which holds an `EndpointId`'s text.
fn endpoint_of(row: &PgRow) -> Result<EndpointId> {
    let text: String = row.try_get("endpoint_id")?;
    text.parse()
        .with_context(|| format!("endpoint_id {text:?} is no node key"))
}

/// The agent in `row`'s `agent` column.
fn agent_of(row: &PgRow) -> Result<AgentSlug> {
    Ok(AgentSlug::new(row.try_get::<String, _>("agent")?))
}

impl Store {
    /// Adds one finished turn to its session's day, `endpoint` being the
    /// node that ran it.
    pub(crate) async fn add_turn_usage(
        &self,
        user: &UserKey,
        endpoint: &EndpointId,
        turn: &TurnUsage,
    ) -> Result<()> {
        name("session", &turn.session, MAX_NAME)?;
        name("agent", turn.agent.as_str(), MAX_NAME)?;
        name("folder", &turn.cwd, MAX_FOLDER)?;
        let cost = turn
            .cost_usd
            .filter(|cost| cost.is_finite() && (0.0..=MAX_TURN_COST).contains(cost));
        let tokens = i64::try_from(turn.tokens)?;
        let unpriced = if cost.is_some() { 0 } else { tokens };
        let at = i64::try_from(turn.at)?;
        let mut tx = self.pool.begin().await?;
        let day: NaiveDate = sqlx::query_scalar(
            "SELECT (to_timestamp($2) AT TIME ZONE
                 COALESCE((SELECT timezone FROM users WHERE id=$1), 'UTC'))::date",
        )
        .bind(user.as_str())
        .bind(at as f64)
        .fetch_one(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO agent_usage (user_id, endpoint_id, session, day, agent, cwd, cost_usd,
                 tokens, unpriced_tokens, turns)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 1)
             ON CONFLICT (user_id, endpoint_id, session, day) DO UPDATE SET
                 cost_usd = CASE WHEN excluded.cost_usd IS NULL THEN agent_usage.cost_usd
                     ELSE COALESCE(agent_usage.cost_usd, 0) + excluded.cost_usd END,
                 tokens = agent_usage.tokens + excluded.tokens,
                 unpriced_tokens = agent_usage.unpriced_tokens + excluded.unpriced_tokens,
                 turns = agent_usage.turns + 1",
        )
        .bind(user.as_str())
        .bind(endpoint.to_string())
        .bind(&turn.session)
        .bind(day)
        .bind(turn.agent.as_str())
        .bind(&turn.cwd)
        .bind(cost)
        .bind(tokens)
        .bind(unpriced)
        .execute(&mut *tx)
        .await?;
        for model in turn.models.iter().take(MAX_WINDOWS) {
            if model.model.is_empty() || model.model.as_str().len() > MAX_NAME {
                continue;
            }
            sqlx::query(
                "INSERT INTO agent_model_usage (user_id, day, agent, model, tokens)
                 VALUES ($1, $2, $3, $4, $5)
                 ON CONFLICT (user_id, day, agent, model) DO UPDATE SET
                     tokens = agent_model_usage.tokens + excluded.tokens",
            )
            .bind(user.as_str())
            .bind(day)
            .bind(turn.agent.as_str())
            .bind(model.model.as_str())
            .bind(i64::try_from(model.tokens)?)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// What the user's sessions spent this week, from Monday, by machine,
    /// folder and agent, today apart.
    pub(crate) async fn week_spend(&self, user: &UserKey) -> Result<Vec<SpendRow>> {
        let rows = sqlx::query(&format!(
            "WITH t AS (SELECT {TODAY} AS today)
             SELECT endpoint_id, cwd, agent, day = t.today AS today, SUM(cost_usd) AS cost,
                 SUM(tokens)::BIGINT AS tokens, SUM(unpriced_tokens)::BIGINT AS unpriced
             FROM agent_usage, t
             WHERE user_id=$1 AND day >= date_trunc('week', t.today)::date AND day <= t.today
             GROUP BY endpoint_id, cwd, agent, day = t.today"
        ))
        .bind(user.as_str())
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
        .bind(user.as_str())
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

    /// Keeps `plan` as read on `endpoint`, unless a fresher report of the
    /// same agent is kept. Says whether it was kept.
    pub(crate) async fn save_plan_usage(
        &self,
        user: &UserKey,
        endpoint: &EndpointId,
        plan: &PlanUsage,
    ) -> Result<bool> {
        name("agent", plan.agent.as_str(), MAX_NAME)?;
        if plan.plan.as_ref().is_some_and(|p| p.len() > MAX_NAME) {
            bail!("the plan's name is too long");
        }
        let windows: Vec<&LimitWindow> = plan
            .windows
            .iter()
            .filter(|w| w.minutes > 0 && w.minutes <= MAX_WINDOW_MINUTES)
            .filter(|w| w.used_percent.is_finite())
            .filter(|w| w.scope.as_ref().is_none_or(|s| s.len() <= MAX_NAME))
            .take(MAX_WINDOWS)
            .collect();
        let read_at = DateTime::<Utc>::from_timestamp(i64::try_from(plan.at)?, 0)
            .unwrap_or_else(Utc::now)
            // A clock ahead of the server's does not win for good.
            .min(Utc::now());
        let saved = sqlx::query(
            "INSERT INTO plan_usage (user_id, agent, endpoint_id, plan, windows, read_at)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (user_id, agent) DO UPDATE SET
                 endpoint_id=excluded.endpoint_id, plan=excluded.plan,
                 windows=excluded.windows, read_at=excluded.read_at
             WHERE excluded.read_at >= plan_usage.read_at",
        )
        .bind(user.as_str())
        .bind(plan.agent.as_str())
        .bind(endpoint.to_string())
        .bind(&plan.plan)
        .bind(Json(&windows))
        .bind(read_at)
        .execute(&self.pool)
        .await?;
        Ok(saved.rows_affected() > 0)
    }

    /// Each agent's plan usage, by agent.
    pub(crate) async fn plan_usages(&self, user: &UserKey) -> Result<Vec<PlanRow>> {
        let rows = sqlx::query(
            "SELECT agent, endpoint_id, plan, windows, read_at FROM plan_usage
             WHERE user_id=$1 ORDER BY agent",
        )
        .bind(user.as_str())
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
