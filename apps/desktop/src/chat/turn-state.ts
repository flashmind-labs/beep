import type { Step, Turn, TurnThought } from "../ui/turn.ts";

// Runtime state follows the conversation turn rather than the mounted view.
export interface TurnState {
  steps: Step[];
  folded: boolean;
  done: boolean;
  user: boolean | null;
  failed: boolean;
  stopped: boolean;
  started: ((runId: string | null) => void) | null;
  agent: string | null;
  model: string | null;
  status: string | null;
  begun: number | null;
  ended: number | null;
  thought: TurnThought | null;
}

const states = new WeakMap<Turn, TurnState>();
export function turnState(turn: Turn): TurnState {
  let current = states.get(turn);
  if (!current) {
    current = { steps: [], folded: true, done: false, user: null, failed: false, stopped: false, started: null, agent: null, model: null, status: null, begun: null, ended: null, thought: null };
    states.set(turn, current);
  }
  return current;
}
