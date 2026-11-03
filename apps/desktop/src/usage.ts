// The pill at the right end of every command center header: how much of
// each coding agent's plan is used and what the agents cost today. A click
// opens the details: the plans, then today, this week or this month by
// project and harness. A server too old to keep usage hides it.

import { agentLabel, el as h } from "./projects-state.ts";

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
/// How often the pill reads the usage again while the window shows.
export const POLL_MS = 60_000;
/// After asking the machines to read their plans, how long to wait before
/// reading what they sent.
export const REFRESH_WAIT_MS = 4_000;

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

/// "Claude" and "Codex" in the pill, where space is short.
export function shortName(agent: string) {
  if (agent === "claude") return "Claude";
  return agentLabel(agent);
}

/// "Max plan" for Claude's `max`, "ChatGPT Pro" for Codex's `pro`.
export function planName(agent: string, plan: string | null) {
  if (!plan) return "";
  const word = plan.replace(/[_-]+/g, " ").replace(/\b\w/g, c => c.toUpperCase());
  if (agent === "codex") return `ChatGPT ${word}`;
  return `${word} plan`;
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
  if (at - now < DAY * 60_000) {
    return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
  }
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

/// "$4.20".
export function money(usd: number) {
  return `$${(Number.isFinite(usd) ? usd : 0).toFixed(2)}`;
}

/// "1.3M", "12K", "800".
export function tokenCount(n: number) {
  if (n >= 100_000) return `${(n / 1_000_000).toFixed(1).replace(/\.0$/, "")}M`;
  if (n >= 1_000) return `${Math.round(n / 1_000)}K`;
  return String(n);
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

/// What a project row adds after its dollars: its API tokens, or on an
/// older server the tokens no price covered. Tokens a plan covered are
/// left out: the plan's bars above say how far it is used.
export function projectExtra(spend: Spend, metered: boolean) {
  if (!metered) return unpricedLine(spend.unpriced);
  return spend.metered_tokens ? `${tokenCount(spend.metered_tokens)}` : "";
}

/// One harness's line in a period: its cost when it reports one, then
/// tokens; tokens only for use a subscription covered.
export function harnessLine(spend: HarnessSpend) {
  const tokens = spend.tokens > 0 ? `${tokenCount(spend.tokens)}` : "";
  if (spend.cost_usd == null || spend.subscription) return tokens;
  return tokens ? `${money(spend.cost_usd)} · ${tokens}` : money(spend.cost_usd);
}

/// Whether there is anything to show: any plan read or any spending.
export function hasUsage(view: UsageView | null): view is UsageView {
  if (!view) return false;
  const spent = (p: Period | null) => !!p && (p.total.cost_usd > 0 || p.tokens > 0 || p.projects.length > 0 || p.harnesses.length > 0);
  return view.limits.some(p => p.windows.length) || spent(view.today) || spent(view.week) || spent(view.month);
}

/// The periods the popover offers: the month only when the server keeps it.
export function periods(view: UsageView) {
  return PERIODS.filter(p => p.key !== "month" || view.month != null);
}

function bar(used: number, cls: string) {
  const i = h("i", used > WARN_USED ? "warn" : "");
  i.style.width = `${used}%`;
  return h("span", cls, i);
}

function pill(view: UsageView) {
  const parts: Node[] = [];
  for (const plan of view.limits) {
    const used = planUsed(plan);
    if (used == null) continue;
    parts.push(h("span", "", `${shortName(plan.agent)} `, h("b", "", `${used}%`), " used"), bar(used, "bar"));
  }
  parts.push(h("span", "", h("b", "", money(view.today.total.cost_usd)), " today"));
  return parts;
}

function limitBlock(plan: PlanLimits, now: number) {
  const name = [agentLabel(plan.agent), planName(plan.agent, plan.plan)].filter(Boolean).join(" · ");
  const head = h("h5", "", h("span", "", name), h("span", "", asOf(plan.as_of, now)));
  head.title = `Read on ${plan.machine_name}`;
  const rows = plan.windows.map(w => {
    const used = windowUsed(w);
    const reset = w.resets_at != null ? ` · resets ${resetName(w.resets_at, now)}` : "";
    return h("div", "us-lim", h("span", "", windowName(w)), bar(used, "bar"), h("small", "", `${used}% used${reset}`));
  });
  return h("div", "", head, ...rows);
}

function spendRow(name: Node | string, cost: string, extra: string, strong = false) {
  const tag = strong ? "b" : "span";
  const right = h(tag, "us-r", cost, extra ? h("small", "", `${cost ? " · " : ""}${extra}`) : null);
  return h("div", "us-row", typeof name === "string" ? h(tag, "", name) : name, right);
}

function periodBlock(view: UsageView, key: PeriodKey, pick: (key: PeriodKey) => void) {
  const period = view[key] ?? view.today;
  const metered = !!view.metered;
  const seg = h("div", "us-seg", ...periods(view).map(p => {
    const b = h("button", p.key === key ? "on" : "", p.label);
    b.type = "button";
    b.setAttribute("aria-pressed", String(p.key === key));
    b.addEventListener("click", () => pick(p.key));
    return b;
  }));
  const block = h("div", "", seg);
  if (period.projects.length) {
    block.append(h("h5", "", h("span", "", "By project"), h("span", "", metered ? "API spend" : "at API prices")));
    for (const s of period.projects) block.append(spendRow(s.project, s.cost_usd > 0 ? money(s.cost_usd) : "", projectExtra(s, metered)));
  } else if (key === "today") {
    block.append(h("div", "us-row", h("small", "", "Nothing yet today")));
  }
  if (period.harnesses.length) {
    block.append(h("h5", period.projects.length ? "us-sub" : "", h("span", "", "By harness")));
    for (const spend of period.harnesses) {
      const row = spendRow(h("small", "", harnessName(spend)), "", harnessLine(spend));
      if (spend.models.length) row.title = spend.models.map(m => `${m.name}: ${tokenCount(m.tokens)} tokens`).join("\n");
      block.append(row);
    }
  }
  const meteredTokens = period.total.metered_tokens ?? 0;
  const tokens = metered ? (meteredTokens ? `${tokenCount(meteredTokens)}` : "")
    : period.tokens ? `${tokenCount(period.tokens)}` : unpricedLine(period.total.unpriced);
  block.append(spendRow("Total", money(period.total.cost_usd), tokens, true));
  return block;
}

/// Moves `pop` back inside the window when its edge would leave it, and
/// lets it scroll when it is taller than the room below.
function keepInWindow(pop: HTMLElement) {
  const r = pop.getBoundingClientRect();
  const gap = 8;
  if (r.right > window.innerWidth - gap) pop.style.transform = `translateX(${window.innerWidth - gap - r.right}px)`;
  else if (r.left < gap) pop.style.transform = `translateX(${gap - r.left}px)`;
  pop.style.maxHeight = `${Math.max(160, window.innerHeight - r.top - gap)}px`;
}

function popover(view: UsageView, key: PeriodKey, pick: (key: PeriodKey) => void, now: number) {
  const blocks: Node[] = view.limits.filter(p => p.windows.length).map(p => limitBlock(p, now));
  blocks.push(
    periodBlock(view, key, pick),
    h("div", "us-note", view.metered
      ? "Dollars are API spend. Plan use shows as tokens."
      : "Dollars are what the same work would cost at API prices."),
  );
  const pop = h("div", "us-pop", ...blocks);
  pop.setAttribute("role", "dialog");
  pop.setAttribute("aria-label", "Plan usage and cost");
  return pop;
}

/// Draws the pill into `host` and keeps it current. `read(refresh)` gets
/// the usage, or null from a server too old to keep it; `refresh` has the
/// machines read their plans again.
export function mountUsage(host: HTMLElement, read: (refresh: boolean) => Promise<UsageView | null>) {
  let view: UsageView | null = null;
  let open = false;
  let period: PeriodKey = "today";
  host.classList.add("us");
  const pick = (key: PeriodKey) => { period = key; draw(); };

  const draw = () => {
    if (!hasUsage(view)) {
      host.hidden = true;
      host.replaceChildren();
      return;
    }
    host.hidden = false;
    const button = h("button", "us-pill", ...pill(view));
    button.type = "button";
    button.title = "Plan usage and cost today";
    button.setAttribute("aria-expanded", String(open));
    button.addEventListener("click", () => toggle(!open));
    const pop = open ? popover(view, period, pick, Date.now()) : null;
    host.replaceChildren(button, ...(pop ? [pop] : []));
    if (pop) keepInWindow(pop);
  };

  const load = async (refresh: boolean) => {
    try {
      view = await read(refresh);
    } catch (e) {
      console.warn("reading usage:", e);
      return;
    }
    draw();
  };

  const toggle = (to: boolean) => {
    open = to;
    draw();
    if (!open) return;
    void load(true);
    setTimeout(() => { if (open) void load(false); }, REFRESH_WAIT_MS);
  };

  document.addEventListener("click", e => {
    if (open && !e.composedPath().includes(host)) toggle(false);
  });
  document.addEventListener("keydown", e => {
    if (open && e.key === "Escape") toggle(false);
  });
  setInterval(() => { if (document.visibilityState === "visible") void load(false); }, POLL_MS);
  void load(false);
}
