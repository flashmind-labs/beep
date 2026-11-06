//! What the user's coding agents spent and their plans' usage, as machines report it.

use super::User;
use crate::store::{DayModelRow, DaySpendRow, ModelRow, Periods, PlanRow, SpendRow};
use anyhow::Result;

impl User {
    /// This week's spending by machine, folder and agent, today apart.
    pub(crate) async fn week_spend(&self) -> Result<Vec<SpendRow>> {
        self.store.week_spend(&self.key).await
    }

    /// Today's tokens by agent and model.
    pub(crate) async fn today_models(&self) -> Result<Vec<ModelRow>> {
        self.store.today_models(&self.key).await
    }

    pub(crate) async fn plan_usages(&self) -> Result<Vec<PlanRow>> {
        self.store.plan_usages(&self.key).await
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
