// The start form's Model field: what it shows for the models an agent
// offers on a machine, as `agent_models` lists them.

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

export interface ModelChoice {
  /// What the start sends; empty for the agent's own default.
  value: string;
  label: string;
  title: string;
}

/// What the Model field shows: nothing, a spinner while the machine is
/// asked, a dropdown, or a box to type a name in.
export type ModelField =
  | { kind: "hidden" }
  | { kind: "loading" }
  | { kind: "list"; choices: ModelChoice[] }
  | { kind: "text"; placeholder: string };

/// How long a machine's list is kept here; the node keeps it as long.
export const MODELS_KEPT_MS = 5 * 60 * 1000;

/// The machine to list models on: the one picked, else the one Auto would
/// pick, when it has said. `null` when there is none yet.
export function modelMachine(machine: string, autoPick: string | null): string | null {
  return machine === "auto" ? autoPick : machine;
}

/// The field for `list`: `undefined` while it is asked, `null` when the
/// machine could not say (a server or node too old, or an error).
export function modelField(list: AgentModels | null | undefined): ModelField {
  if (list === undefined) return { kind: "loading" };
  if (!list || list.pick === "fixed") return { kind: "hidden" };
  const name = (id: string) => list.models.find(m => m.id === id)?.name ?? id;
  if (list.pick === "named" || !list.models.length) {
    return { kind: "text", placeholder: list.current ? `Default (${list.current})` : "Default" };
  }
  const first: ModelChoice = {
    value: "",
    label: list.current ? `Default (${name(list.current)})` : "Default",
    title: "The model the agent runs when none is picked",
  };
  return {
    kind: "list",
    choices: [first, ...list.models.map(m => ({
      value: m.id,
      label: m.name || m.id,
      title: [m.name && m.name !== m.id ? m.id : "", m.description ?? ""].filter(Boolean).join(": "),
    }))],
  };
}
