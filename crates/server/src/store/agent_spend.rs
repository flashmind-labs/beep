//! What agent sessions spent per their logs, and daily spend this month including watched turns no log covers.

use super::{
    Store,
    agent_usage::{agent_of, endpoint_of},
};
use crate::{
    account::user::UserKey,
    protocol::{AgentSlug, ModelId, node::usage::Billing},
};
use anyhow::Result;
use chrono::NaiveDate;
use iroh::EndpointId;
use sqlx::{Row, postgres::PgRow};

/// The user's timezone: `$1` is the user's id.
const TIMEZONE: &str = "COALESCE((SELECT timezone FROM users WHERE id=$1), 'UTC')";

/// The days the periods start on, in the user's timezone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Periods {
    pub(crate) today: NaiveDate,
    /// Monday.
    pub(crate) week: NaiveDate,
    /// The first of the month.
    pub(crate) month: NaiveDate,
}

impl Periods {
    /// The earliest day any period counts.
    pub(crate) fn start(&self) -> NaiveDate {
        self.week.min(self.month)
    }
}

/// What one folder's sessions on one machine spent with one agent on one day.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DaySpendRow {
    pub(crate) endpoint: EndpointId,
    pub(crate) cwd: String,
    pub(crate) agent: AgentSlug,
    pub(crate) day: NaiveDate,
    /// How it was paid for; `Unknown` for the turns Beep watched.
    pub(crate) billing: Billing,
    /// `None` when none of it was priced.
    pub(crate) cost_usd: Option<f64>,
    pub(crate) tokens: u64,
    pub(crate) unpriced_tokens: u64,
}

/// The tokens one model of one agent used on one day.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DayModelRow {
    pub(crate) agent: AgentSlug,
    pub(crate) model: ModelId,
    pub(crate) day: NaiveDate,
    /// How it was paid for; `Unknown` for the turns Beep watched.
    pub(crate) billing: Billing,
    pub(crate) tokens: u64,
    /// From the agents' logs, rather than the turns Beep watched.
    pub(crate) from_logs: bool,
}

fn billing_of(row: &PgRow) -> Result<Billing> {
    Ok(Billing::from_name(&row.try_get::<String, _>("billing")?))
}

impl Store {
    /// Today, Monday and the first of the month, in the user's timezone.
    pub(crate) async fn usage_periods(&self, user: &UserKey) -> Result<Periods> {
        let row = sqlx::query(&format!(
            "WITH t AS (SELECT (now() AT TIME ZONE {TIMEZONE})::date AS today)
             SELECT today, date_trunc('week', today)::date AS week,
                 date_trunc('month', today)::date AS month FROM t"
        ))
        .bind(user.as_str())
        .fetch_one(&self.pool)
        .await?;
        Ok(Periods {
            today: row.try_get("today")?,
            week: row.try_get("week")?,
            month: row.try_get("month")?,
        })
    }

    /// Daily spend over `periods` by machine, folder, agent and billing, from logs plus uncovered watched turns.
    pub(crate) async fn day_spend(
        &self,
        user: &UserKey,
        periods: &Periods,
    ) -> Result<Vec<DaySpendRow>> {
        let rows = sqlx::query(&format!(
            "WITH tz AS (SELECT {TIMEZONE} AS name),
             h AS (
                 SELECT s.endpoint_id, s.cwd, s.agent, s.session, s.billing,
                     (s.hour AT TIME ZONE tz.name)::date AS day,
                     SUM(s.cost_usd) AS cost,
                     SUM(s.input_tokens + s.output_tokens + s.cache_read_tokens
                         + s.cache_write_tokens) AS tokens,
                     SUM(s.unpriced_tokens) AS unpriced
                 FROM agent_session_usage s, tz
                 WHERE s.user_id=$1 AND s.hour >= ($2::date::timestamp AT TIME ZONE tz.name)
                 GROUP BY 1, 2, 3, 4, 5, 6)
             SELECT endpoint_id, cwd, agent, billing, day, SUM(cost) AS cost,
                 SUM(tokens)::BIGINT AS tokens, SUM(unpriced)::BIGINT AS unpriced
             FROM (
                 SELECT endpoint_id, cwd, agent, billing, day,
                     CASE WHEN tokens > unpriced THEN cost END AS cost, tokens, unpriced
                 FROM h
                 UNION ALL
                 SELECT u.endpoint_id, u.cwd, u.agent, 'unknown', u.day, u.cost_usd, u.tokens,
                     u.unpriced_tokens
                 FROM agent_usage u
                 WHERE u.user_id=$1 AND u.day >= $2 AND NOT EXISTS (
                     SELECT 1 FROM h WHERE h.endpoint_id=u.endpoint_id AND h.agent=u.agent
                         AND h.session=u.agent_session AND h.day=u.day)
             ) x
             WHERE day >= $2 AND day <= $3
             GROUP BY endpoint_id, cwd, agent, billing, day"
        ))
        .bind(user.as_str())
        .bind(periods.start())
        .bind(periods.today)
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|row| {
                Ok(DaySpendRow {
                    endpoint: endpoint_of(row)?,
                    cwd: row.try_get("cwd")?,
                    agent: agent_of(row)?,
                    day: row.try_get("day")?,
                    billing: billing_of(row)?,
                    cost_usd: row.try_get("cost")?,
                    tokens: u64::try_from(row.try_get::<i64, _>("tokens")?)?,
                    unpriced_tokens: u64::try_from(row.try_get::<i64, _>("unpriced")?)?,
                })
            })
            .collect()
    }

    /// Tokens per model per day of `periods`, by agent, from logs and watched turns.
    pub(crate) async fn day_models(
        &self,
        user: &UserKey,
        periods: &Periods,
    ) -> Result<Vec<DayModelRow>> {
        let rows = sqlx::query(&format!(
            "WITH tz AS (SELECT {TIMEZONE} AS name)
             SELECT agent, model, billing, day, tokens, from_logs FROM (
                 SELECT s.agent, s.model, s.billing,
                     (s.hour AT TIME ZONE tz.name)::date AS day,
                     SUM(s.input_tokens + s.output_tokens + s.cache_read_tokens
                         + s.cache_write_tokens)::BIGINT AS tokens,
                     true AS from_logs
                 FROM agent_session_usage s, tz
                 WHERE s.user_id=$1 AND s.hour >= ($2::date::timestamp AT TIME ZONE tz.name)
                 GROUP BY 1, 2, 3, 4
                 UNION ALL
                 SELECT agent, model, 'unknown', day, tokens, false FROM agent_model_usage
                 WHERE user_id=$1 AND day >= $2
             ) x
             WHERE day >= $2 AND day <= $3
             ORDER BY tokens DESC"
        ))
        .bind(user.as_str())
        .bind(periods.start())
        .bind(periods.today)
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|row| {
                Ok(DayModelRow {
                    agent: agent_of(row)?,
                    model: ModelId::new(row.try_get::<String, _>("model")?),
                    day: row.try_get("day")?,
                    billing: billing_of(row)?,
                    tokens: u64::try_from(row.try_get::<i64, _>("tokens")?)?,
                    from_logs: row.try_get("from_logs")?,
                })
            })
            .collect()
    }
}
