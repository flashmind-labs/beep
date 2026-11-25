// What a project's bar and page have picked: the tile and run on the page, the machine,
// checkout, model and target in the bar.
import { DEFAULT_CHECKOUT, type BaseBranches } from "../agents/agentpick.ts";
import type { Picked } from "../agents/agentpicker.tsx";
import type { AgentHost } from "../agents/agents-state.ts";
import { signal } from "../ui/signal.ts";
import type { BarPlace, BarTarget } from "./projectbar.ts";

/// The row of the project's list picked, null for the first, and a run to
/// open on the Actions tile.
/// The tile picked, a run to open, and whether the project's board is blank,
/// which centers the agent bar on the page under the day's painting.
export const pa = { tile: null as string | null, run: null as { project: string; id: number; host: string } | null, blank: false };

/// The bar under a project: it starts a coding agent in the project's checkout,
/// by default as it is (the agent makes its own branch), and opens its session.
export const pc = {
  project: null as string | null,
  /// The machine picked; the bar starts on another while it has no checkout
  /// and agent there.
  machine: "",
  /// The machine the project is being added to, until it is ready.
  adding: null as AgentHost | null,
  checkout: DEFAULT_CHECKOUT,
  branches: new Map<string, BaseBranches | null>(),
  /// True while the bar's files upload and its start is handed over.
  starting: false,
  /// The harness, model and thinking picked, and the machine and harness
  /// the model was picked for, as `machine/agent`.
  picked: { agent: "", model: "", effort: null, fast: null, query: "" } as Picked,
  modelFor: "",
  /// Whether the picker shows.
  open: false,
  /// What Work on it or Work on PR put in the bar, until it starts or is
  /// cleared.
  target: null as BarTarget | null,
};


/// The project's page or bar changed: a pick, a list or a store behind them.
export const projectChanged = signal();

/// Follows the place the bar's input points at: the empty agent view binds its pane to it.
export const barPlaced = new Set<(place: BarPlace | null) => void>();
