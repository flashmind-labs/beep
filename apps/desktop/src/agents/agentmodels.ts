// The models an agent offers on a machine, as `agent_models` lists them.

export interface ModelInfo {
  id: string;
  name: string;
  description: string | null;
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

/// How long a machine's list is kept here; the node keeps it as long.
export const MODELS_KEPT_MS = 5 * 60 * 1000;

export interface SessionPick { model: string; effort: string | null }

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
          super.set(id, { model: pick.model, effort: pick.effort });
        }
      }
    } catch { /* Storage can be unavailable. */ }
  }

  override set(id: string, pick: SessionPick): this {
    super.set(id, pick);
    try { localStorage.setItem(this.key, JSON.stringify([...this].slice(-500))); } catch { /* Keep in memory. */ }
    return this;
  }

  override clear(): void {
    super.clear();
    try { localStorage.removeItem(this.key); } catch { /* Keep in memory. */ }
  }

  override delete(id: string): boolean {
    const removed = super.delete(id);
    try { localStorage.setItem(this.key, JSON.stringify([...this].slice(-500))); } catch { /* Keep in memory. */ }
    return removed;
  }
}
