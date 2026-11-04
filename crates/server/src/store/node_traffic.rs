//! Each node's lifetime traffic, from the cumulative counters of `Up::Traffic`.

use super::Store;
use crate::{
    protocol::{MachineTraffic, TrafficCounter, node::traffic::TrafficReport},
    user::UserKey,
};
use anyhow::{Context, Result};
use iroh::EndpointId;
use sqlx::Row;
use std::time::SystemTime;

/// The most counters kept from one report, and the longest key.
const MAX_COUNTERS: usize = 64;
const MAX_KEY: usize = 128;

impl Store {
    /// Adds what `report` has over the last one. A report from an earlier
    /// process than the stored one, late off a dying connection, is ignored.
    pub(crate) async fn save_node_traffic(
        &self,
        user: &UserKey,
        endpoint: &EndpointId,
        report: &TrafficReport,
    ) -> Result<()> {
        let started_at =
            i64::try_from(report.started_at_ms).context("the start time is out of range")?;
        let (keys, values): (Vec<&str>, Vec<i64>) = report
            .counters
            .iter()
            .filter(|(key, _)| key.len() <= MAX_KEY)
            .filter_map(|(key, value)| Some((key.as_str(), i64::try_from(*value).ok()?)))
            .take(MAX_COUNTERS)
            .unzip();
        sqlx::query(
            "INSERT INTO node_traffic AS t (node, key, value, run_started_at, run_value)
             SELECT $1, n.key, n.value, $3, n.value
             FROM unnest($4::text[], $5::bigint[]) AS n(key, value)
             WHERE EXISTS (SELECT 1 FROM nodes WHERE endpoint_id=$1 AND user_id=$2)
             ON CONFLICT (node, key) DO UPDATE SET
                 value = t.value + CASE WHEN excluded.run_started_at = t.run_started_at
                     THEN GREATEST(excluded.run_value - t.run_value, 0)
                     ELSE excluded.run_value END,
                 run_value = CASE WHEN excluded.run_started_at = t.run_started_at
                     THEN GREATEST(excluded.run_value, t.run_value)
                     ELSE excluded.run_value END,
                 run_started_at = excluded.run_started_at,
                 updated_at = now()
             WHERE excluded.run_started_at >= t.run_started_at",
        )
        .bind(endpoint.to_string())
        .bind(user.as_str())
        .bind(started_at)
        .bind(&keys)
        .bind(&values)
        .execute(&self.pool)
        .await
        .context("the node's traffic was not saved")?;
        Ok(())
    }

    /// The lifetime totals of `user`'s machines.
    pub(crate) async fn machine_traffic(&self, user: &UserKey) -> Result<Vec<MachineTraffic>> {
        let rows = sqlx::query(
            "SELECT t.node, t.key, t.value, t.updated_at
             FROM node_traffic t JOIN nodes n ON n.endpoint_id = t.node
             WHERE n.user_id=$1 ORDER BY t.node, t.key",
        )
        .bind(user.as_str())
        .fetch_all(&self.pool)
        .await
        .context("the machines' traffic was not read")?;
        let mut machines: Vec<MachineTraffic> = Vec::new();
        for row in rows {
            let id: String = row.try_get(0)?;
            let counter = TrafficCounter {
                key: row.try_get(1)?,
                bytes: u64::try_from(row.try_get::<i64, _>(2)?)?,
            };
            let updated_at: SystemTime = row.try_get::<chrono::DateTime<chrono::Utc>, _>(3)?.into();
            match machines.last_mut() {
                Some(machine) if machine.id == id => {
                    machine.updated_at = machine.updated_at.max(updated_at);
                    machine.counters.push(counter);
                }
                _ => machines.push(MachineTraffic {
                    id,
                    updated_at,
                    counters: vec![counter],
                }),
            }
        }
        Ok(machines)
    }
}
