import { createElement } from "react";
import { placeWindowParts } from "./window-root.ts";
import { windowHost } from "./window-shell.tsx";
import { UsageWidget } from "./usage-view.tsx";
import { subscribeChanges, topic } from "../ui/mesh-events.ts";
import { AMBIENT_MS, ambient } from "../ui/active.ts";
import type { WrittenCredits } from "../ui/credits.ts";
export { creditAmount, creditScope, type ProviderCredits } from "../ui/credits.ts";
// The pill at the right end of every command center header: how much of each
// coding agent's plan is used and what the agents cost today.

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
  project: string;
  cost_usd: number;
  unpriced: NamedTokens[];
  /// Tokens of metered use; 0 from a server that prices everything.
  metered_tokens?: number;
  /// Tokens a subscription covered, by agent.
  subscription?: NamedTokens[];
}

export interface HarnessSpend {
  agent: string;
  /// A subscription covered it: tokens only, no dollars.
  subscription?: boolean;
  cost_usd: number | null;
  tokens: number;
  models: NamedTokens[];
}

/// What was spent in one period, by project and by harness.
export interface Period {
  total: Spend;
  tokens: number;
  projects: Spend[];
  harnesses: HarnessSpend[];
}

export interface UsageView {
  limits: PlanLimits[];
  today: Period;
  week: Period;
  /// Null from a server from before the monthly report.
  month: Period | null;
  /// The dollars are metered API spend only; false from an older server,
  /// whose dollars price every token at API prices.
  metered?: boolean;
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

/// The share of a window used, 0 to 100, rounded.
export function windowUsed(w: Pick<UsageWindow, "used_percent">) {
  const used = Number.isFinite(w.used_percent) ? w.used_percent : 0;
  return Math.round(Math.min(100, Math.max(0, used)));
}

/// How much of a plan is used: its tightest window, or null without any.
export function planUsed(plan: { windows: Pick<UsageWindow, "used_percent">[] }): number | null {
  if (!plan.windows.length) return null;
  return Math.max(...plan.windows.map(windowUsed));
}

/// One item in the pill: a harness's binding window, the one most used, or
/// one of its model limits, such as Fable's weekly window.
export interface PillPlan {
  agent: string;
  /// The model a limit applies to; null for the plan as a whole.
  model: string | null;
  used: number;
  window: string;
}

/// Each plan with windows, as the pill shows it: the plan's binding window,
/// then each model limit used past 1%.
export function pillPlans(limits: PlanLimits[]): PillPlan[] {
  return limits.flatMap(plan => {
    const binding = plan.windows.filter(w => !w.scope)
      .reduce<UsageWindow | null>((most, w) => !most || windowUsed(w) > windowUsed(most) ? w : most, null);
    const models = plan.windows.filter(w => w.scope && windowUsed(w) > 1)
      .map(w => ({ agent: plan.agent, model: w.scope, used: windowUsed(w), window: windowName(w) }));
    return binding ? [{ agent: plan.agent, model: null, used: windowUsed(binding), window: windowName(binding) }, ...models] : models;
  });
}

/// Whether a period's rows show any dollars, so the note on dollars applies.
export function showsDollars(period: Period) {
  return period.total.cost_usd > 0 || period.projects.some(p => p.cost_usd > 0) || period.harnesses.some(h => harnessLine(h).includes("$"));
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
  const base = w.minutes === 5 * HOUR ? "5 hours"
    : w.minutes === WEEK ? "This week"
    : w.minutes % DAY === 0 ? `${w.minutes / DAY} days`
    : w.minutes % HOUR === 0 ? `${w.minutes / HOUR} hours`
    : `${w.minutes} min`;
  if (!w.scope) return base;
  return `${base}, ${w.scope.charAt(0).toUpperCase()}${w.scope.slice(1)}`;
}

/// "14:30" for a reset within a day, else the weekday ("Mon").
export function resetName(at: number, now: number) {
  const d = new Date(at);
  if (at - now < DAY * 60_000) return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
  return d.toLocaleDateString("en-US", { weekday: "short" });
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

/// "1.1M Codex tokens", one agent after another.
export function unpricedLine(unpriced: NamedTokens[]) {
  return unpriced.filter(t => t.tokens > 0).map(t => `${tokenCount(t.tokens)} ${shortName(t.name)} tokens`).join(", ");
}

/// "Claude Code" for metered use, "Claude Code, subscription" for use a
/// plan covered.
export function harnessName(spend: Pick<HarnessSpend, "agent" | "subscription">) {
  return spend.subscription ? `${agentLabel(spend.agent)}, subscription` : agentLabel(spend.agent);
}

/// What a project row adds after its dollars: its tokens, or on an older
/// server the tokens no price covered.
export function projectExtra(spend: Spend, metered: boolean) {
  if (!metered) return unpricedLine(spend.unpriced);
  const tokens = (spend.subscription ?? []).reduce((sum, t) => sum + t.tokens, spend.metered_tokens ?? 0);
  return tokens ? tokenCount(tokens) : "";
}

/// One harness's line in a period: its cost when it reports one, then
/// tokens. Use a subscription covered shows its estimate at API prices
/// as `~$61`, when there is one.
export function harnessLine(spend: HarnessSpend) {
  const tokens = spend.tokens > 0 ? `${tokenCount(spend.tokens)}` : "";
  if (spend.cost_usd == null || (spend.subscription && spend.cost_usd < 0.5)) return tokens;
  const cost = spend.subscription ? `~$${Math.round(spend.cost_usd)}` : money(spend.cost_usd);
  return tokens ? `${cost} · ${tokens}` : cost;
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
  return view.limits.some(p => p.windows.length || p.credits?.length) || spent(view.today) || spent(view.week) || spent(view.month);
}

let allProjects = false;

/// Draws the header's usage pill and keeps it current.
export function mountUsage(read: (refresh: boolean) => Promise<UsageView | null>) {
  let view: UsageView | null = null;
  let loaded = false;
  let open = false;
  let period: PeriodKey = "today";
  const pick = (key: PeriodKey) => { period = key; draw(); };
  const draw = () => {
    if (!hasUsage(view)) { placeWindowParts({ usage: null }); return; }
    const current = view;
    placeWindowParts({ usage: createElement(UsageWidget, { view: current, open, period, allProjects,
      toggle: () => toggle(!open), pick, more: () => { allProjects = !allProjects; draw(); } }) });
  };

  const load = async (refresh: boolean) => {
    let next: UsageView | null;

    try {
      next = await read(refresh);
    } catch (e) {
      console.warn("reading usage:", e);
      return;
    }

    if (loaded && JSON.stringify(next) === JSON.stringify(view)) return;
    loaded = true;
    view = next;
    draw();
  };

  const toggle = (to: boolean) => {
    open = to;
    draw();
    if (!open) return;
    void load(true);
  };

  document.addEventListener("click", e => { if (open && !e.composedPath().includes(windowHost("usage"))) toggle(false); });
  document.addEventListener("keydown", e => { if (open && e.key === "Escape") toggle(false); });
  // Spending changes with every turn; the pill follows at most once a second.
  const reload = ambient(() => load(false), AMBIENT_MS);
  subscribeChanges(topic("usage"), { deliver: "latest", receive: () => reload() });
  void load(false);
}
