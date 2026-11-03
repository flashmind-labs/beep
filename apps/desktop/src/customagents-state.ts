// What the Custom agents list of a project page decides, as pure functions
// so they can be tested without a window.

/// One custom agent as `custom_agents` gives it (CustomAgentRow).
export interface CustomAgentRow {
  name: string;
  /// `claude`, `opencode`, `codex`, `pi` or `flsh`.
  harness: string;
  scope: "project" | "user";
  description: string;
  model: string | null;
  tools: string[];
  color: string | null;
  /// Relative to the project folder, or starting `~/` for a user one.
  path: string;
  prompt: string;
  prompt_cut: boolean;
  /// Why the file could not be read; the row then shows it.
  error: string | null;
}

/// The machine and folder the list is read on.
export interface AgentsPlace {
  id: string;
  name: string;
  folder: string;
}

/// The machine the list is read on: this Mac's checkout first, else the
/// first machine that has the project's folder. Null when none has it.
export function agentsPlace(rows: readonly { kind: string; id: string; name: string; folder: string | null; mac?: boolean }[]): AgentsPlace | null {
  const here = rows.filter(r => r.kind === "here" && r.id && r.folder);
  const pick = here.find(r => r.mac) || here[0];
  return pick ? { id: pick.id, name: pick.name, folder: pick.folder! } : null;
}

/// What Start puts in the bar. The node runs Claude Code and the others
/// through ACP, which has no way to pick a named agent, so the prompt asks
/// for it; each harness hands the work to the agent it names.
export function agentPrompt(name: string) {
  return `Use the ${name} agent to `;
}

/// The colours Claude Code names, as the window draws them.
const COLORS: Record<string, string> = {
  red: "#E5534B",
  orange: "#F0883E",
  yellow: "#D4A72C",
  green: "#3FB950",
  cyan: "#39C5CF",
  blue: "#3B82F6",
  purple: "#A371F7",
  pink: "#EC6CB9",
};

/// A CSS colour for an agent's `color`: a name above or a hex colour; null
/// for anything else, so file text never reaches a style.
export function agentColor(color: string | null | undefined): string | null {
  const c = (color || "").trim().toLowerCase();
  if (Object.hasOwn(COLORS, c)) return COLORS[c];
  return /^#(?:[0-9a-f]{3}|[0-9a-f]{6})$/.test(c) ? c : null;
}

/// The line under an agent's name: where it is defined, its model and how
/// many tools it may use.
export function agentMeta(a: Pick<CustomAgentRow, "scope" | "model" | "tools">): string[] {
  const out = [a.scope === "user" ? "user" : "project"];
  if (a.model) out.push(a.model);
  if (a.tools.length) out.push(a.tools.length === 1 ? a.tools[0] : `${a.tools.length} tools`);
  return out;
}
