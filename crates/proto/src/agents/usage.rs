//! Usage and spend reports.

use crate::{AgentSlug, ModelId};
use std::time::SystemTime;

wire_structs! {
    /// What `GetUsageReport` gives.
    pub struct UsageReport {
        /// As in `UsageView`.
        pub limits: Vec<PlanLimits>,
        pub today: PeriodSpend,
        /// From Monday.
        pub week: PeriodSpend,
        /// From the first of the month.
        pub month: PeriodSpend,
    }

    /// What coding agents spent over one period.
    pub struct PeriodSpend {
        /// Every project together, `project` empty.
        pub total: ProjectSpend,
        /// Every token, priced or not.
        pub tokens: u64,
        /// By project, the most expensive first, only those that spent.
        pub projects: Vec<ProjectSpend>,
        /// By harness, the most expensive first, only those that spent.
        pub harnesses: Vec<HarnessSpend>,
    }

    /// What `GetUsage` gives.
    pub struct UsageView {
        /// One per agent whose plan a machine reported, the freshest report
        /// of any machine: Claude Code, then Codex.
        pub limits: Vec<PlanLimits>,
        /// Today's spending in the caller's timezone, by project, the most
        /// expensive first.
        pub today: Vec<ProjectSpend>,
        /// Today's spending by harness, only those that spent anything.
        pub harnesses: Vec<HarnessSpend>,
        /// The whole week so far, from Monday.
        pub week: ProjectSpend,
    }

    /// A coding agent's plan usage and provider balances.
    pub struct PlanLimits {
        /// The agent, such as `claude`, `codex` or `flsh`.
        pub agent: AgentSlug,
        /// The plan, such as `max` or `pro`, when the agent says: free-form
        /// text the agent reports.
        pub plan: Option<String>,
        /// The machine that read it.
        pub machine_name: String,
        /// When it was read.
        pub as_of: SystemTime,
        /// The shortest window first.
        pub windows: Vec<PlanWindow>,
        /// Provider balances, separate from subscription windows and session cost.
        #[serde(default)]
        pub credits: Vec<ProviderCredits>,
    }

    /// One rate limit window of a plan.
    pub struct PlanWindow {
        /// How long the window is: 300 for five hours, 10080 for a week.
        pub minutes: u32,
        /// How much of it is used, from 0 to 100.
        pub used_percent: f64,
        pub resets_at: Option<SystemTime>,
        /// What it is limited to, such as `Opus`; `None` for the whole plan.
        /// Free-form text the agent reports.
        pub scope: Option<String>,
    }

    /// What coding agents spent in one project, or in all of them.
    pub struct ProjectSpend {
        /// `org/repo`, else the folder's name; empty for a total.
        pub project: String,
        /// At API prices, in US dollars: an estimate on a flat-rate plan.
        pub cost_usd: f64,
        /// The tokens of the agents that report no cost, such as Codex.
        pub unpriced: Vec<AgentTokens>,
    }

    /// The tokens one agent used.
    pub struct AgentTokens {
        pub agent: AgentSlug,
        pub tokens: u64,
    }

    /// What one coding agent harness spent today.
    pub struct HarnessSpend {
        /// Its slug, such as `claude`.
        pub agent: AgentSlug,
        /// At API prices, in US dollars, when it reports a cost; `None` for
        /// one that reports tokens only, such as Codex.
        pub cost_usd: Option<f64>,
        /// Every token its turns used.
        pub tokens: u64,
        /// The tokens by model, the most first, when it says.
        pub models: Vec<ModelSpend>,
    }

    /// The tokens one model used.
    pub struct ModelSpend {
        pub model: ModelId,
        pub tokens: u64,
    }

    /// What `GetSpendReport` gives.
    pub struct SpendReport {
        /// As in `UsageView`.
        pub limits: Vec<PlanLimits>,
        pub today: SpendPeriod,
        /// From Monday.
        pub week: SpendPeriod,
        /// From the first of the month.
        pub month: SpendPeriod,
    }

    /// What coding agents spent over one period, metered use apart from use a
    /// subscription covered.
    pub struct SpendPeriod {
        /// Metered API spend in US dollars, every project together.
        pub cost_usd: f64,
        /// The tokens of metered use, priced or not.
        pub metered_tokens: u64,
        /// The tokens a subscription covered.
        pub subscription_tokens: u64,
        /// By project, the most expensive first, then the most tokens, only
        /// those that spent.
        pub projects: Vec<ProjectBill>,
        /// By harness and billing, the most expensive first, then the most
        /// tokens, only those that spent.
        pub harnesses: Vec<HarnessBill>,
    }

    /// What coding agents spent in one project.
    pub struct ProjectBill {
        /// `org/repo`, else the folder's name.
        pub project: String,
        /// Metered API spend in US dollars.
        pub cost_usd: f64,
        /// The tokens of metered use, priced or not.
        pub metered_tokens: u64,
        /// The tokens a subscription covered, by agent.
        pub subscription: Vec<AgentTokens>,
    }

    /// What one harness spent one way: metered, or on a subscription.
    pub struct HarnessBill {
        /// Its slug, such as `claude`.
        pub agent: AgentSlug,
        /// A subscription covered it: tokens only, `cost_usd` is `None` in a
        /// server's reply.
        pub subscription: bool,
        /// Metered API spend in US dollars; `None` when none of it was priced
        /// or a subscription covered it. The desktop's own report (never sent
        /// by a server) fills it on a subscription bill with the estimate at
        /// API prices, which is in no total.
        pub cost_usd: Option<f64>,
        pub tokens: u64,
        /// The tokens by model, the most first, when it says.
        pub models: Vec<ModelSpend>,
    }
}

ffi_structs! {
    /// A provider's credit snapshot, with decimal amounts encoded as strings.
    pub struct ProviderCredits {
        pub provider: String,
        pub scope: CreditScope,
        pub unit: CreditUnit,
        pub remaining: Option<String>,
        pub total: Option<String>,
        pub used: Option<String>,
        pub resets: Option<String>,
    }
}

/// What a provider's balance applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CreditScope {
    Account,
    ApiKey,
    Budget,
    #[serde(other)]
    Unknown,
}

/// The unit of a provider's balance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CreditUnit {
    Usd,
    Credits,
    #[serde(other)]
    Unknown,
}
