// What the usage pill reads and how it names it.
import type { WrittenCredits } from "../ui/credits.ts";
import { agentLabel } from "../project/projects-state.ts";
import { money, tokenCount } from "../ui/format.ts";

export interface UsageWindow {
  minutes: number;
  used_percent: number;
  resets_at: number | null;
  scope: string | null;
}

export interface PlanLimits {
  agent: string;
  /// The agent's named account; null for its default one.
  account: string | null;
  plan: string | null;
  machine_name: string;
  as_of: number;
  windows: UsageWindow[];
  credits?: WrittenCredits[];
}

export interface NamedTokens {
  name: string;
  tokens: number;
}

export interface Spend {
  cost_usd: number;
}

export interface HarnessSpend {
  agent: string;
  /// A subscription covered it: tokens only, no dollars.
  subscription?: boolean;
  cost_usd: number | null;
  tokens: number;
  models: NamedTokens[];
}

export interface UsageTableRow {
  first: string | null;
  last: string | null;
  project: string | null;
  agent: string | null;
  tokens: number;
  cost_usd: number;
}

/// What was spent in one period, by project and by harness.
export interface Period {
  daily: UsageTableRow[];
  total: Spend;
  tokens: number;
  projects: Spend[];
  harnesses: HarnessSpend[];
}

export interface UsageView {
  historical: UsageTableRow[];
  limits: PlanLimits[];
  today: Period;
  week: Period;
  month: Period;
}

export type PeriodKey = "today" | "week" | "month";

export const PERIODS: { key: PeriodKey; label: string }[] = [
  { key: "today", label: "Today" },
  { key: "week", label: "This week" },
  { key: "month", label: "This month" },
];

/// Above this much used, a bar turns amber.
export const WARN_USED = 70;

const HOUR = 60;
const DAY = 24 * HOUR;
const WEEK = 7 * DAY;
export const FIVE_HOURS = 5 * HOUR;

/// The share of a window used, 0 to 100, rounded.
export function windowUsed(w: Pick<UsageWindow, "used_percent">) {
  const used = Number.isFinite(w.used_percent) ? w.used_percent : 0;
  return Math.round(Math.min(100, Math.max(0, used)));
}

/// One item in the pill for a harness or one of its model limits.
export interface PillPlan {
  agent: string;
  /// The agent's named account; null for its default one.
  account: string | null;
  /// The model a limit applies to; null for the plan as a whole.
  model: string | null;
  windows: UsageWindow[];
}

/// The highest longer-window usage, followed by the highest five-hour usage.
function pillWindows(windows: UsageWindow[]) {
  let usage: UsageWindow | null = null;
  let fiveHour: UsageWindow | null = null;

  for (const window of windows) {
    if (window.minutes === FIVE_HOURS) {
      if (!Number.isFinite(window.used_percent) || window.used_percent < 1) continue;
      if (!fiveHour || windowUsed(window) > windowUsed(fiveHour)) fiveHour = window;
    } else if (!usage || windowUsed(window) > windowUsed(usage)) {
      usage = window;
    }
  }

  return [usage, fiveHour].filter(window => window !== null);
}

export function pillPlans(limits: PlanLimits[]): PillPlan[] {
  return limits.flatMap(plan => {
    if (!plan.windows.length) return [];

    const { agent, account } = plan;
    const unscoped = plan.windows.filter(window => !window.scope);
    // Plans with only model groups summarize those groups under the harness mark.
    const windows = pillWindows(unscoped.length ? unscoped : plan.windows);
    const items: PillPlan[] = windows.length ? [{ agent, account, model: null, windows }] : [];
    if (!unscoped.length) return items;

    const models = new Map<string, UsageWindow[]>();
    for (const window of plan.windows) {
      if (!window.scope) continue;
      const windows = models.get(window.scope);
      if (windows) windows.push(window);
      else models.set(window.scope, [window]);
    }

    for (const [model, windows] of models) {
      if (windows.some(window => windowUsed(window) > 1)) {
        items.push({ agent, account, model, windows: pillWindows(windows) });
      }
    }

    return items;
  });
}

/// Whether a period's rows show any dollars, so the note on dollars applies.
export function showsDollars(period: Period) {
  return period.total.cost_usd > 0 || period.projects.some(p => p.cost_usd > 0) || period.harnesses.some(h => harnessCost(h) !== "");
}

/// "Claude" and "Codex" in the pill, where space is short.
export function shortName(agent: string) {
  return agent === "claude" ? "Claude" : agentLabel(agent);
}

/// "Max plan" for Claude's `max`, "ChatGPT Pro" for Codex's `pro`.
export function planName(agent: string, plan: string | null) {
  if (!plan) return "";
  const word = plan.replace(/[_-]+/g, " ").replace(/\b\w/g, c => c.toUpperCase());
  return agent === "codex" ? `ChatGPT ${word}` : `${word} plan`;
}

/// "5 hours", "This week", "This week, Opus".
export function windowName(w: Pick<UsageWindow, "minutes" | "scope">) {
  const base = w.minutes === FIVE_HOURS ? "5 hours"
    : w.minutes === WEEK ? "This week"
    : w.minutes % DAY === 0 ? `${w.minutes / DAY} days`
    : w.minutes % HOUR === 0 ? `${w.minutes / HOUR} hours`
    : `${w.minutes} min`;
  if (!w.scope) return base;
  return `${base}, ${w.scope.charAt(0).toUpperCase()}${w.scope.slice(1)}`;
}

/// "14:30" for a reset within a day, else the weekday ("Mon").
export function resetName(at: number, now: number) {
  const min = Math.max(1, Math.ceil((at - now) / 60_000));
  const days = Math.floor(min / DAY), hours = Math.floor((min % DAY) / HOUR), mins = min % HOUR;
  const parts: [number, string][] = days > 0 ? [[days, "d"], [hours, "h"]] : hours > 0 ? [[hours, "h"], [mins, "m"]] : [[mins, "m"]];

  return `in ${parts.filter(([n]) => n > 0).map(([n, unit]) => `${n}${unit}`).join(" ")}`;
}

/// "as of 4 min ago".
export function asOf(at: number, now: number) {
  const min = Math.max(0, Math.floor((now - at) / 60_000));
  if (min < 1) return "as of just now";
  if (min < HOUR) return `as of ${min} min ago`;
  const hours = Math.floor(min / HOUR);
  if (hours < 24) return `as of ${hours} h ago`;
  return `as of ${Math.floor(hours / 24)} d ago`;
}

/// "Claude Code" for metered use, "Claude Code, subscription" for use a
/// plan covered.
export function harnessName(spend: Pick<HarnessSpend, "agent" | "subscription">) {
  return spend.subscription ? `${agentLabel(spend.agent)}, subscription` : agentLabel(spend.agent);
}

/// Metered dollars, or the API estimate for usage covered by a plan.
export function harnessCost(spend: HarnessSpend) {
  if (spend.cost_usd == null || (spend.subscription && spend.cost_usd < 0.5)) return "";
  return spend.subscription ? `~$${Math.round(spend.cost_usd)}` : money(spend.cost_usd);
}

/// Tokens reported for each model across harnesses and billing types.
export function modelTotals(harnesses: HarnessSpend[]): NamedTokens[] {
  const totals = new Map<string, number>();

  for (const harness of harnesses) {
    for (const model of harness.models) {
      if (model.tokens > 0) totals.set(model.name, (totals.get(model.name) ?? 0) + model.tokens);
    }
  }

  return [...totals].map(([name, tokens]) => ({ name, tokens }))
    .sort((a, b) => b.tokens - a.tokens || a.name.localeCompare(b.name));
}

/// What the pill says of today: the dollars paid, else the tokens plans covered.
export function todayLine(today: { total: Pick<Spend, "cost_usd">; tokens: number }) {
  return today.total.cost_usd > 0 || !today.tokens ? money(today.total.cost_usd) : tokenCount(today.tokens);
}

/// Whether there is anything to show: any plan read or any spending.
export function hasUsage(view: UsageView | null): view is UsageView {
  if (!view) return false;
  const spent = (p: Period | null) => !!p && (p.total.cost_usd > 0 || p.tokens > 0 || p.projects.length > 0 || p.harnesses.length > 0);
  return !!view.historical?.length || view.limits.some(p => p.windows.length || p.credits?.length) || spent(view.today) || spent(view.week) || spent(view.month);
}
