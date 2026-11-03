//! What the user's coding agents spent and how much of their plans is
//! left, as their machines report it.

use super::User;
use crate::{
    protocol::node::usage::{PlanUsage, SessionSpend, TurnUsage},
    store::{DayModelRow, DaySpendRow, ModelRow, Periods, PlanRow, SpendRow},
};
use anyhow::Result;
use iroh::EndpointId;

impl User {
    /// Adds a finished turn of a session on the machine `endpoint`.
    pub(crate) async fn add_turn_usage(
        &self,
        endpoint: &EndpointId,
        turn: &TurnUsage,
    ) -> Result<()> {
        self.store.add_turn_usage(&self.key, endpoint, turn).await
    }

    /// Keeps a plan's usage as `endpoint` read it, unless a fresher one is
    /// kept.
    pub(crate) async fn save_plan_usage(
        &self,
        endpoint: &EndpointId,
        plan: &PlanUsage,
    ) -> Result<bool> {
        self.store.save_plan_usage(&self.key, endpoint, plan).await
    }

    /// This week's spending by machine, folder and agent, today apart.
    pub(crate) async fn week_spend(&self) -> Result<Vec<SpendRow>> {
        self.store.week_spend(&self.key).await
    }

    /// Today's tokens by agent and model.
    pub(crate) async fn today_models(&self) -> Result<Vec<ModelRow>> {
        self.store.today_models(&self.key).await
    }

    /// Each agent's plan usage.
    pub(crate) async fn plan_usages(&self) -> Result<Vec<PlanRow>> {
        self.store.plan_usages(&self.key).await
    }

    /// Keeps what a session on the machine `endpoint` spent, as its log
    /// says, in place of what was kept of it.
    pub(crate) async fn save_session_spend(
        &self,
        endpoint: &EndpointId,
        spend: &SessionSpend,
    ) -> Result<()> {
        self.store
            .save_session_spend(&self.key, endpoint, spend)
            .await
    }

    /// Today, Monday and the first of the month, in the user's timezone.
    pub(crate) async fn usage_periods(&self) -> Result<Periods> {
        self.store.usage_periods(&self.key).await
    }

    /// Each day's spending of `periods` by machine, folder and agent.
    pub(crate) async fn day_spend(&self, periods: &Periods) -> Result<Vec<DaySpendRow>> {
        self.store.day_spend(&self.key, periods).await
    }

    /// Each day's tokens of `periods` by agent and model.
    pub(crate) async fn day_models(&self, periods: &Periods) -> Result<Vec<DayModelRow>> {
        self.store.day_models(&self.key, periods).await
    }
}
