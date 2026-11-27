//! Usage and spend reports.

use crate::{AccountName, AgentSlug, ModelId};
use std::time::SystemTime;

wire_structs! {
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
        /// The agent's named account it is for; `None` for the default one.
        #[serde(default)]
        pub account: Option<AccountName>,
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

    /// The tokens one agent used.
    pub struct AgentTokens {
        pub agent: AgentSlug,
        pub tokens: u64,
    }

    /// The tokens one model used.
    pub struct ModelSpend {
        pub model: ModelId,
        pub tokens: u64,
    }

    /// Spend by project, harness and model.
    pub struct SpendReport {
        /// One per agent whose plan a machine reported, the freshest report
        /// of any machine: Claude Code, then Codex.
        pub limits: Vec<PlanLimits>,
        pub today: SpendPeriod,
        /// From Monday.
        pub week: SpendPeriod,
        /// From the first of the month.
        pub month: SpendPeriod,
        #[serde(default)]
        pub historical: Vec<UsageTableRow>,
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
        #[serde(default)]
        pub daily: Vec<UsageTableRow>,
        /// Every project, harness, model and billing combination that spent, the
        /// most expensive first, for filtering the tables above.
        #[serde(default)]
        pub cells: Vec<SpendCell>,
    }

    /// What one harness spent on one model in one project, one way.
    pub struct SpendCell {
        /// `org/repo`, else the folder's name; `None` for other folders.
        pub project: Option<String>,
        pub agent: AgentSlug,
        pub model: ModelId,
        /// A subscription covered it: the cost is an estimate, in no total.
        pub subscription: bool,
        pub cost_usd: f64,
        pub tokens: u64,
    }

    /// A daily project total or an undated session balance with known local dates.
    pub struct UsageTableRow {
        pub first: Option<String>,
        pub last: Option<String>,
        pub project: Option<String>,
        pub agent: Option<AgentSlug>,
        pub cost_usd: f64,
        pub tokens: u64,
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
        /// A subscription covered it: tokens only.
        pub subscription: bool,
        /// Metered API spend in US dollars; `None` when none of it was
        /// priced. On a subscription bill it holds the estimate at API
        /// prices, which is in no total.
        pub cost_usd: Option<f64>,
        pub tokens: u64,
        /// The tokens by model, the most first, when it says.
        pub models: Vec<ModelSpend>,
    }
}

wire_structs! {
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
