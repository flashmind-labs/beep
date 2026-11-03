// The pill at the right end of every command center header: how much of
// each coding agent's plan is left and what the agents cost today. A click
// opens the details. A server too old to keep usage hides it.

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
}

export interface HarnessSpend {
  agent: string;
  cost_usd: number | null;
  tokens: number;
  models: NamedTokens[];
}

export interface UsageView {
  limits: PlanLimits[];
  today: Spend[];
  harnesses: HarnessSpend[];
  week: Spend;
}

/// Below this much left, a bar turns amber.
export const WARN_LEFT = 30;
/// How often the pill reads the usage again while the window shows.
export const POLL_MS = 60_000;
/// After asking the machines to read their plans, how long to wait before
/// reading what they sent.
export const REFRESH_WAIT_MS = 4_000;

const HOUR = 60;
const DAY = 24 * HOUR;
const WEEK = 7 * DAY;

/// The share of a window still free, 0 to 100, rounded.
export function windowLeft(w: Pick<UsageWindow, "used_percent">) {
  const left = 100 - (Number.isFinite(w.used_percent) ? w.used_percent : 0);
  return Math.round(Math.min(100, Math.max(0, left)));
}

/// What a plan has left: the tightest of its windows, or null without any.
export function planLeft(plan: { windows: Pick<UsageWindow, "used_percent">[] }): number | null {
  if (!plan.windows.length) return null;
  return Math.min(...plan.windows.map(windowLeft));
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

/// Today's cost and the tokens no price covers, over every project.
export function todayTotal(today: Spend[]) {
  return {
    cost: today.reduce((n, s) => n + s.cost_usd, 0),
    tokens: today.reduce((n, s) => n + s.unpriced.reduce((m, t) => m + t.tokens, 0), 0),
  };
}

/// One harness's line in Today: its cost when it reports one, then tokens.
export function harnessLine(spend: HarnessSpend) {
  const tokens = spend.tokens > 0 ? `${tokenCount(spend.tokens)} tokens` : "";
  if (spend.cost_usd == null) return tokens;
  return tokens ? `${money(spend.cost_usd)} · ${tokens}` : money(spend.cost_usd);
}

/// Whether there is anything to show: any plan read or any spending.
export function hasUsage(view: UsageView | null): view is UsageView {
  if (!view) return false;
  return view.limits.some(p => p.windows.length) || view.today.length > 0 || view.harnesses.length > 0 || view.week.cost_usd > 0;
}

function bar(left: number, cls: string) {
  const i = h("i", left < WARN_LEFT ? "warn" : "");
  i.style.width = `${left}%`;
  return h("span", cls, i);
}

function pill(view: UsageView) {
  const parts: Node[] = [];
  for (const plan of view.limits) {
    const left = planLeft(plan);
    if (left == null) continue;
    parts.push(h("span", "", `${shortName(plan.agent)} `, h("b", "", `${left}%`)), bar(left, "bar"));
  }
  parts.push(h("span", "", h("b", "", money(todayTotal(view.today).cost)), " today"));
  return parts;
}

function limitBlock(plan: PlanLimits, now: number) {
  const name = [agentLabel(plan.agent), planName(plan.agent, plan.plan)].filter(Boolean).join(" · ");
  const head = h("h5", "", h("span", "", name), h("span", "", asOf(plan.as_of, now)));
  head.title = `Read on ${plan.machine_name}`;
  const rows = plan.windows.map(w => {
    const left = windowLeft(w);
    const reset = w.resets_at != null ? ` · resets ${resetName(w.resets_at, now)}` : "";
    return h("div", "us-lim", h("span", "", windowName(w)), bar(left, "bar"), h("small", "", `${left}% left${reset}`));
  });
  return h("div", "", head, ...rows);
}

function spendRow(name: Node | string, cost: string, extra: string, strong = false) {
  const tag = strong ? "b" : "span";
  const right = h(tag, "", cost, extra ? h("small", "", `${cost ? " · " : ""}${extra}`) : null);
  return h("div", "us-row", typeof name === "string" ? h(tag, "", name) : name, right);
}

function popover(view: UsageView, now: number) {
  const blocks: Node[] = view.limits.filter(p => p.windows.length).map(p => limitBlock(p, now));
  const total = todayTotal(view.today);
  const today = h("div", "", h("h5", "", h("span", "", "Today"), h("span", "", "at API prices")));
  for (const s of view.today) today.append(spendRow(s.project, s.cost_usd > 0 ? money(s.cost_usd) : "", unpricedLine(s.unpriced)));
  if (!view.today.length) today.append(h("div", "us-row", h("small", "", "Nothing yet today")));
  for (const spend of view.harnesses) {
    const row = spendRow(h("small", "", agentLabel(spend.agent)), "", harnessLine(spend));
    if (spend.models.length) row.title = spend.models.map(m => `${m.name}: ${tokenCount(m.tokens)} tokens`).join("\n");
    today.append(row);
  }
  today.append(
    spendRow("Total", money(total.cost), total.tokens ? `${tokenCount(total.tokens)} tokens` : "", true),
    spendRow("This week", money(view.week.cost_usd), unpricedLine(view.week.unpriced)),
  );
  blocks.push(today, h("div", "us-note", "Plans are flat rate: the dollars are what the same work would cost at API prices. Agents that report only tokens show tokens."));
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
  host.classList.add("us");

  const draw = () => {
    if (!hasUsage(view)) {
      host.hidden = true;
      host.replaceChildren();
      return;
    }
    host.hidden = false;
    const button = h("button", "us-pill", ...pill(view));
    button.type = "button";
    button.title = "Plan usage left and cost today";
    button.setAttribute("aria-expanded", String(open));
    button.addEventListener("click", () => toggle(!open));
    host.replaceChildren(button, ...(open ? [popover(view, Date.now())] : []));
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
