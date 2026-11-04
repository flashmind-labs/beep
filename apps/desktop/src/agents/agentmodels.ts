// The models an agent offers on a machine, as `agent_models` lists them.

export interface ModelInfo {
  id: string;
  name: string;
  description: string | null;
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
