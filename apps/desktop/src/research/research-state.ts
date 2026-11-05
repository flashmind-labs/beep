// An opened research: what the page holds of it, and the pure parts of what
// it says about the team. Drawing is in panes.ts, the page itself in open.ts.
import { plural } from "../ui/format.ts";
import type { ResearchStatus as Status } from "./research-list.ts";

/// A member of the team (Rust's `AgentView`).
export interface Agent {
  id: string;
  name: string;
  role: string;
  model: string;
  /// A scout's round; 0 for agents in every round. Rust always sends it;
  /// only the stand-in for an unknown id lacks it.
  round?: number;
}

/// A source the scouts kept (Rust's `SourceView`).
export interface Source {
  number: number;
  url: string;
  title: string;
  quote: string;
  agent: string;
}

export type Said = { kind: "said"; to: string | null; content: string };
export type StepEntry = { kind: "step"; label: string; success: boolean };
export type Note = { kind: "note"; content: string; sources: number[] };
/// What an agent did (Rust's `EntryView`).
export type Entry = { seq: number; agent: string; at: number } & (Said | StepEntry | Note);

/// A research as Rust sends it (`ResearchView`).
export interface ResearchView {
  topic: string;
  status: Status;
  agents: Agent[];
  entries: Entry[];
  sources: Source[];
  /// The paper as Markdown, for copying and export.
  draft: string;
  /// The paper as sanitized HTML.
  draft_html: string;
  started_at: number;
  finished_at: number | null;
  /// The newest round, from 1.
  round: number;
}
/// A research as kept here: its entries are in `Team.entries`.
export type Research = Omit<ResearchView, "entries">;

/// A message of the lead's chat (Rust's `Turn`), as far as this reads it.
export type Turn =
  | { role: "user"; text: string }
  | { role: "assistant"; html: string }
  | { role: "scheduled"; label: string }
  | { role: "approval" };

/// What an agent is doing: a line and its state.
export type Doing = [line: string, state: "run" | "ok" | "wait" | ""];

/// The research and everything its agents did.
export interface Team {
  research: Research | null;
  entries: readonly Entry[];
}

/// What the page holds of the research it shows.
export interface Open extends Team {
  session: string;
  /// The newest entry held, which the next read asks after.
  after: number;
  turns: Turn[];
  /// The agents' threads and the room open as tabs, by agent id or `room`.
  tabs: string[];
  /// The tab on screen; null follows the research: the paper once written, else the agents.
  tab: string | null;
  /// The round the agents tab shows; null follows the newest.
  pick: number | null;
  /// Whether the round menu is open.
  menu: boolean;
  /// The lead's reply streaming in, as HTML.
  sending: string | null;
  error: string;
  exporting: boolean;
}

export const RUNNING: ReadonlySet<Status | undefined> = new Set<Status>(["researching", "reviewing", "writing"]);
const SCOUT_COLORS = ["var(--ok)", "var(--link)", "var(--warn)", "color-mix(in srgb, var(--ok) 50%, var(--link))", "color-mix(in srgb, var(--warn) 50%, var(--hot))"];

/// "1 scout", "2 scouts".
export const count = (n: number, word: string) => plural(n, word, `${word}s`);

/// The letter and color of an agent's tile.
export function look(id: string) {
  if (id === "lead") return { ini: "L", c: "var(--hot)" };
  if (id === "critic") return { ini: "R", c: "color-mix(in srgb, var(--accent) 60%, var(--fg))" };
  if (id === "writer") return { ini: "W", c: "var(--fg)" };
  const n = +id.replace("scout-", "") || 1;
  return { ini: String(n), c: SCOUT_COLORS[(n - 1) % SCOUT_COLORS.length] };
}

export const agentOf = (t: Team, id: string): Agent => t.research?.agents.find(a => a.id === id) ?? { id, name: id, role: "", model: "" };

/// Which members work in each phase; the others have finished or wait.
const PHASE: Record<string, string[]> = { researching: ["scout"], reviewing: ["critic", "scout"], writing: ["writer", "critic"] };
/// What an agent is, without its number: `scout`, `critic`, `writer` or `lead`.
export const kind = (id: string) => id.startsWith("scout-") ? "scout" : id;
export const round = (t: Team) => t.research?.round ?? 1;
/// A scout from an earlier round, done while a later one works.
export const earlier = (t: Team, id: string) => kind(id) === "scout" && (agentOf(t, id).round ?? 1) < round(t);

/// When the newest round started: its scouts' first entry, else the research's start.
export function roundStart(t: Team) {
  const r = t.research;
  if (!r || round(t) === 1) return r?.started_at ?? Date.now();
  const first = t.entries.find(e => kind(e.agent) === "scout" && agentOf(t, e.agent).round === round(t));
  return first?.at ?? r.started_at;
}

/// "14 min", at least one.
export function minutes(from: number, to: number) {
  return `${Math.max(1, Math.round((to - from) / 60000))} min`;
}

/// What an agent is doing: a line and its state (run, ok, wait or none).
export function doing(t: Team, id: string): Doing {
  const st = t.research?.status ?? "planning";
  const mine = t.entries.filter(e => e.agent === id);
  const notes = mine.filter(e => e.kind === "note").length;
  const sources = t.research?.sources.filter(s => s.agent === id).length ?? 0;
  const said = mine.filter(e => e.kind === "said" && e.to).length;
  const summary = ({ scout: `Done: ${count(notes, "note")}, ${count(sources, "source")}`, critic: `Done: ${count(said, "challenge")}`, writer: "Done: the paper", lead: "Posted the result" } as Record<string, string>)[kind(id)];
  if (id === "lead") {
    if (st === "planning") return ["Waiting for your go", "wait"];
    if (st === "reviewing") return [round(t) > 1 ? `Coordinating round ${round(t)}` : "Coordinating", "run"];
    if (RUNNING.has(st)) return ["Watching the room", "run"];
    return st === "done" ? [summary, "ok"] : ["", ""];
  }
  if (st === "done") return [summary, "ok"];
  if (!RUNNING.has(st)) return mine.length ? [summary.replace("Done: ", ""), ""] : ["", ""];
  if (earlier(t, id)) return [summary, "ok"];
  const order = ["researching", "reviewing", "writing"];
  if (PHASE[st].includes(kind(id))) return [mine.findLast(e => e.kind === "step")?.label ?? "Starting", "run"];
  const starts = ({ critic: "reviewing", writer: "writing", scout: "researching" } as Record<string, string>)[kind(id)];
  if (order.indexOf(starts) > order.indexOf(st)) return [kind(id) === "critic" ? "Starts after the scouts" : "Starts after review", ""];
  return [summary, "ok"];
}

/// The line under the chat while the team works.
export function progress(t: Team, now: number) {
  const r = t.research!, st = r.status;
  const scouts = r.agents.filter(a => kind(a.id) === "scout" && !earlier(t, a.id));
  const took = minutes(roundStart(t), now);
  if (st === "researching") {
    const busy = scouts.filter(a => doing(t, a.id)[1] === "run").length;
    const n = `${busy} of ${count(scouts.length, "scout")} working`;
    return round(t) > 1 ? `Round ${round(t)}: ${n}, ${took}` : `Researching: ${n}, ${took}`;
  }
  if (st === "reviewing") return `The critic and the lead are reviewing, ${took}`;
  return round(t) > 1 ? `The writer is revising the paper, ${took}` : `The writer is drafting, ${took}`;
}

/// The facts in the head, after the status: the round, how long it took, the sources.
export function facts(t: Team) {
  const r = t.research;
  return [
    r?.status === "interrupted" && "Server restarted",
    round(t) > 1 && `Round ${round(t)}`,
    r?.status === "done" && `took ${minutes(roundStart(t), r.finished_at ?? Date.now())}`,
    r?.sources.length && count(r.sources.length, "source"),
  ].filter(Boolean).join(" · ");
}

/// A link's host without `www.`, or the link itself when it is not one.
export function host(url: string) {
  try { return new URL(url).host.replace(/^www\./, ""); } catch { return url; }
}

/// The tab that shows: the one picked while it is open, else the paper once
/// written and the agents before.
export function shownTab(o: Open) {
  const fixed = ["draft", "notes", "sources", "agents"];
  if (o.tab && (fixed.includes(o.tab) || o.tabs.includes(o.tab))) return o.tab;
  return o.research?.draft ? "draft" : "agents";
}

/// `tab` opened, and on screen.
export function openTab(o: Open, tab: string) {
  if (!o.tabs.includes(tab)) o.tabs.push(tab);
  o.tab = tab;
}

/// `tab` closed; the agents show in its place if it was on screen.
export function closeTab(o: Open, tab: string) {
  o.tabs = o.tabs.filter(t => t !== tab);
  if (o.tab === tab) o.tab = "agents";
}
