// The models an agent offers on a machine, as `agent_models` lists them.

export interface ModelInfo {
  id: string;
  name: string;
  description: string | null;
  /// Who serves it, such as `openrouter`, when its id names one; the same name can
  /// show under several providers.
  provider: string | null;
  efforts?: string[];
  fast?: boolean | null;
  context_window?: number | null;
  max_context_window?: number | null;
}

/// The models an agent can run on a machine. `listed`: pick from `models`;
/// `named`: type any name; `fixed`: none can be picked.
export interface AgentModels {
  pick: "listed" | "named" | "fixed";
  current: string | null;
  models: ModelInfo[];
}

export interface SessionPick { model: string; effort: string | null; fast: boolean | null }

/// Keep session choices across window reloads, including resolved defaults.
export class SessionPicks extends Map<string, SessionPick> {
  private readonly key = "beep.sessionModels.v1";

  constructor() {
    super();
    try {
      const saved: unknown = JSON.parse(localStorage.getItem(this.key) || "[]");
      if (!Array.isArray(saved)) return;

      for (const entry of saved) {
        if (!Array.isArray(entry) || entry.length !== 2) continue;
        const [id, pick] = entry;
        if (typeof id === "string" && pick && typeof pick.model === "string"
          && (pick.effort === null || typeof pick.effort === "string")) {
          super.set(id, { model: pick.model, effort: pick.effort, fast: typeof pick.fast === "boolean" ? pick.fast : null });
        }
      }
    } catch { /* Storage can be unavailable. */ }
  }

  override set(id: string, pick: SessionPick): this {
    super.set(id, pick);
    this.save();
    return this;
  }

  override clear(): void {
    super.clear();
    try { localStorage.removeItem(this.key); } catch { /* Keep in memory. */ }
  }

  override delete(id: string): boolean {
    const removed = super.delete(id);
    this.save();
    return removed;
  }

  private save() {
    try { localStorage.setItem(this.key, JSON.stringify([...this].slice(-500))); } catch { /* Keep in memory. */ }
  }
}

/// The harness, model and thinking picked for a project.
export interface ProjectPick extends SessionPick { agent: string }

const PROJECT_PICKS = "beep.projectModels.v1";

/// Read on each use so another window's choices are included.
function projectPicks(): Map<string, ProjectPick> {
  const picks = new Map<string, ProjectPick>();

  try {
    const saved: unknown = JSON.parse(localStorage.getItem(PROJECT_PICKS) ?? "[]");
    if (!Array.isArray(saved)) return picks;

    for (const entry of saved) {
      if (!Array.isArray(entry) || entry.length !== 2) continue;
      const [project, pick]: readonly unknown[] = entry;
      if (typeof project === "string" && pick !== null && typeof pick === "object"
        && "agent" in pick && typeof pick.agent === "string"
        && "model" in pick && typeof pick.model === "string"
        && "effort" in pick && (pick.effort === null || typeof pick.effort === "string")) {
        picks.set(project, { agent: pick.agent, model: pick.model, effort: pick.effort, fast: "fast" in pick && typeof pick.fast === "boolean" ? pick.fast : null });
      }
    }
  } catch { /* Storage can be unavailable. */ }

  return picks;
}

export function lastProjectPick(project: string | null): ProjectPick | null {
  return projectPicks().get(project ?? "") ?? null;
}

export function keepProjectPick(project: string | null, pick: ProjectPick): void {
  if (!pick.agent) return;
  const picks = projectPicks();
  picks.set(project ?? "", pick);

  try {
    localStorage.setItem(PROJECT_PICKS, JSON.stringify([...picks]));
  } catch { /* The current form keeps its choice. */ }
}

const PROJECT_ACCOUNTS = "beep.projectAccounts.v1";

/// The account picked for each project and agent, as `project/agent`.
function projectAccounts(): Map<string, string> {
  try {
    const saved: unknown = JSON.parse(localStorage.getItem(PROJECT_ACCOUNTS) ?? "{}");
    if (saved === null || typeof saved !== "object" || Array.isArray(saved)) return new Map();
    return new Map(Object.entries(saved).filter((entry): entry is [string, string] => typeof entry[1] === "string"));
  } catch { return new Map(); }
}

/// The named account a start of `agent` in `project` runs as, of `names`,
/// the agent's accounts: the one last picked there while the agent still has
/// it; null for the default account.
export function startAccount(names: readonly string[], project: string | null, agent: string | null): string | null {
  const picked = projectAccount(project, agent);
  return names.find(name => name === picked) ?? null;
}

/// The saved choice before checking whether the account is still available.
export function projectAccount(project: string | null, agent: string | null): string | null {
  return agent ? projectAccounts().get(`${project ?? ""}/${agent}`) ?? null : null;
}

/// Keeps `name` as the account `agent` starts as in `project`; null for the default one.
export function keepProjectAccount(project: string | null, agent: string, name: string | null): void {
  const picks = projectAccounts(), key = `${project ?? ""}/${agent}`;
  if (name) picks.set(key, name); else picks.delete(key);

  try {
    localStorage.setItem(PROJECT_ACCOUNTS, JSON.stringify(Object.fromEntries(picks)));
  } catch { /* The current form keeps its choice. */ }
}
