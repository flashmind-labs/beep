// First run, Set up this computer: what this computer has of what Beep's agents need, and what
// Set up does next. firstrun-setup.ts runs it.
import type { MachineEnv } from "../../screens/machines/machines.tsx";

/// What the setup checks, in the order Set up works through them.
export type Item = "git" | "node" | "claude" | "codex";
export const ITEMS: readonly Item[] = ["git", "node", "claude", "codex"];
export type Agent = Extract<Item, "claude" | "codex">;

export const ITEM_NAMES: Record<Item, string> = { git: "Git", node: "Node.js", claude: "Claude Code", codex: "Codex" };
export const ITEM_ROLES: Record<Item, string> = {
  git: "Projects and commits", node: "Needed to install the agents", claude: "Coding agent", codex: "Coding agent",
};

/// What Set up is doing with an item now.
export type Work = { kind: "idle" } | { kind: "installing" } | { kind: "dialog" } | { kind: "signing_in" } | { kind: "failed"; reason: string };

export interface Setup {
  env: MachineEnv | null;
  skipped: ReadonlySet<Item>;
  /// Installed by Set up, before the machine's next report shows it.
  installed: ReadonlySet<Item>;
  work: Readonly<Record<Item, Work>>;
  running: boolean;
}

/// How a row reads.
export type Row =
  | { kind: "installed"; version: string | null }
  | { kind: "signed_in"; account: string | null }
  | { kind: "signed_out" }
  | { kind: "missing" }
  | { kind: "skipped" }
  | { kind: "installing" }
  | { kind: "dialog" }
  | { kind: "signing_in" }
  | { kind: "failed"; reason: string };

export const isAgent = (item: Item): item is Agent => item === "claude" || item === "codex";

function version(env: MachineEnv | null, item: Item): string | null | false {
  const tool = env?.tools.find(t => t.name === item);
  return tool ? tool.version : false;
}

function signedIn(env: MachineEnv | null, agent: Agent): { account: string | null } | null {
  const login = env?.logins.find(l => l.tool === agent && l.signed_in);
  return login ? { account: login.account } : null;
}

export function row(s: Setup, item: Item): Row {
  const work = s.work[item];
  const reported = version(s.env, item);
  const found = reported === false && s.installed.has(item) ? null : reported;
  // The system's own installer finished while Set up went on.
  if (work.kind === "dialog" && found !== false) return { kind: "installed", version: found };
  if (work.kind !== "idle") return work;
  if (s.skipped.has(item)) return { kind: "skipped" };
  if (found === false) return { kind: "missing" };
  if (!isAgent(item)) return { kind: "installed", version: found };
  const login = signedIn(s.env, item);
  return login ? { kind: "signed_in", account: login.account } : { kind: "signed_out" };
}

/// The items Set up still has to install or sign in.
export function pending(s: Setup): Item[] {
  return ITEMS.filter(item => {
    const r = row(s, item);
    return r.kind === "missing" || r.kind === "signed_out" || r.kind === "failed";
  });
}

/// Whether nothing is left to do, so the setup need not show.
export const ready = (s: Setup) => pending(s).length === 0 && ITEMS.every(item => !["installing", "dialog", "signing_in"].includes(row(s, item).kind));

export const idleWork = (): Record<Item, Work> => ({ git: { kind: "idle" }, node: { kind: "idle" }, claude: { kind: "idle" }, codex: { kind: "idle" } });
