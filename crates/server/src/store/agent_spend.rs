//! What coding agent sessions spent as the agents' own logs on the user's
//! machines tell it (`SessionSpend`, see `crate::nodes::usage`), and what
//! the user's agents spent each day of this month: the logs, and the turns
//! Beep watched (`agent_usage`) that no log of the same session covers.

use super::{
    Store,
    agent_usage::{agent_of, endpoint_of},
};
use crate::{
    protocol::{
        AgentSlug, ModelId,
        node::usage::{Billing, MAX_SESSION_HOURS, SessionSpend},
    },
    user::UserKey,
};
use anyhow::{Result, bail};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use iroh::EndpointId;
use sqlx::{Row, postgres::PgRow};

/// The longest agent, model or session name kept, and the longest folder.
const MAX_NAME: usize = 200;
const MAX_FOLDER: usize = 4096;
/// The most one session may cost in an hour; more is a report gone wrong.
const MAX_HOUR_COST: f64 = 10_000.0;

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

/// What the sessions in one folder of one machine spent with one agent on
/// one day.
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

fn name(what: &str, value: &str, max: usize) -> Result<()> {
    if value.is_empty() || value.len() > max {
        bail!("the {what} is empty or too long");
    }
    Ok(())
}

impl Store {
    /// Keeps what a session on `endpoint` spent, in place of what was kept
    /// of it from the report's `since` on.
    pub(crate) async fn save_session_spend(
        &self,
        user: &UserKey,
        endpoint: &EndpointId,
        spend: &SessionSpend,
    ) -> Result<()> {
        name("agent", spend.agent.as_str(), MAX_NAME)?;
        let endpoint = endpoint.to_string();
        name("session", &spend.session, MAX_NAME)?;
        if spend.cwd.len() > MAX_FOLDER {
            bail!("the folder is too long");
        }
        if spend.hours.len() > MAX_SESSION_HOURS {
            bail!("too many hours in one report");
        }
        let since = DateTime::<Utc>::from_timestamp(i64::try_from(spend.since)?, 0)
            .unwrap_or(DateTime::<Utc>::UNIX_EPOCH);
        let latest = Utc::now() + Duration::days(1);
        let mut hours = Vec::new();
        let mut models = Vec::new();
        let mut input = Vec::new();
        let mut output = Vec::new();
        let mut cache_read = Vec::new();
        let mut cache_write = Vec::new();
        let mut costs = Vec::new();
        let mut unpriced = Vec::new();
        let mut billings = Vec::new();
        for hour in &spend.hours {
            let Some(at) = i64::try_from(hour.hour)
                .ok()
                .and_then(|at| DateTime::<Utc>::from_timestamp(at, 0))
                .filter(|at| *at <= latest)
            else {
                continue;
            };
            if hour.model.as_str().is_empty()
                || hour.model.as_str().len() > MAX_NAME
                || !hour.cost_usd.is_finite()
                || !(0.0..=MAX_HOUR_COST).contains(&hour.cost_usd)
            {
                continue;
            }
            hours.push(at);
            models.push(hour.model.as_str());
            input.push(i64::try_from(hour.input_tokens)?);
            output.push(i64::try_from(hour.output_tokens)?);
            cache_read.push(i64::try_from(hour.cache_read_tokens)?);
            cache_write.push(i64::try_from(hour.cache_write_tokens)?);
            costs.push(hour.cost_usd);
            unpriced.push(i64::try_from(hour.unpriced_tokens)?);
            billings.push(hour.billing.as_str());
        }
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "DELETE FROM agent_session_usage
             WHERE user_id=$1 AND endpoint_id=$2 AND agent=$3 AND session=$4 AND hour >= $5",
        )
        .bind(user.as_str())
        .bind(&endpoint)
        .bind(spend.agent.as_str())
        .bind(&spend.session)
        .bind(since)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO agent_session_usage (user_id, endpoint_id, agent, session, cwd, hour,
                 model, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
                 cost_usd, unpriced_tokens, billing)
             SELECT $1, $2, $3, $4, $5, h.hour, h.model, h.input, h.output, h.cache_read,
                 h.cache_write, h.cost, h.unpriced, h.billing
             FROM UNNEST($6::timestamptz[], $7::text[], $8::bigint[], $9::bigint[],
                 $10::bigint[], $11::bigint[], $12::float8[], $13::bigint[], $14::text[])
                 AS h(hour, model, input, output, cache_read, cache_write, cost, unpriced,
                     billing)
             ON CONFLICT (user_id, endpoint_id, agent, session, hour, model, billing)
             DO UPDATE SET
                 cwd=excluded.cwd, input_tokens=excluded.input_tokens,
                 output_tokens=excluded.output_tokens,
                 cache_read_tokens=excluded.cache_read_tokens,
                 cache_write_tokens=excluded.cache_write_tokens,
                 cost_usd=excluded.cost_usd, unpriced_tokens=excluded.unpriced_tokens",
        )
        .bind(user.as_str())
        .bind(&endpoint)
        .bind(spend.agent.as_str())
        .bind(&spend.session)
        .bind(&spend.cwd)
        .bind(&hours)
        .bind(&models)
        .bind(&input)
        .bind(&output)
        .bind(&cache_read)
        .bind(&cache_write)
        .bind(&costs)
        .bind(&unpriced)
        .bind(&billings)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

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

    /// What the user's agents spent each day of `periods`, by machine,
    /// folder, agent and billing: from the logs, and from the turns Beep
    /// watched of sessions no log covers that day.
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

    /// The tokens each model used each day of `periods`, by agent, from
    /// the logs and from the turns Beep watched apart.
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
