// The models each machine's agents offer, held for the pickers and the model settings.
import type { AgentModels } from "./agentmodels.ts";
import { subscribeEach, topic } from "../ui/mesh-events.ts";
import { pushedEach } from "../ui/snapshot-state.ts";
import { invoke, logError } from "../ui/tauri.ts";

/// The models each machine's agents offer, by `machine/agent`: `null` when it
/// could not say, `undefined` in `list` until the first answer. Each is read once; the
/// `machines` and `accounts` pushes keep it after that.
export const MODELS = new Map<string, ModelsEntry>();
export interface ModelsEntry { list: AgentModels | null | undefined; at: number; status: "idle" | "loading" | "ready" | "failed"; waiting: (() => void)[] }

/// Where the lists are kept between launches; both windows share it.
const MODELS_STORE = "beep.agentModels.v1";
/// Lists older than this are not shown again after a restart.
const MODELS_STORE_MAX_MS = 14 * 24 * 60 * 60 * 1000;

function restoreModels() {
  let kept: Record<string, { list: AgentModels; at: number }> = {};
  try { kept = JSON.parse(localStorage.getItem(MODELS_STORE) || "{}") || {}; } catch { return; }
  const now = Date.now();

  for (const [key, v] of Object.entries(kept)) {
    if (v && v.list && typeof v.at === "number" && now - v.at < MODELS_STORE_MAX_MS) {
      // Shown at once, and asked again the first time it is wanted.
      MODELS.set(key, { list: v.list, at: 0, status: "idle", waiting: [] });
    }
  }
}

function storeModels() {
  const out: Record<string, { list: AgentModels; at: number }> = {};
  for (const [key, v] of MODELS) if (v.list) out[key] = { list: v.list, at: v.at || Date.now() };
  try { localStorage.setItem(MODELS_STORE, JSON.stringify(out)); } catch { /* kept for this launch only */ }
}

restoreModels();

/// Asks `machine` for `agent`'s models unless a request is out; `loaded`
/// runs once the answer comes, including a failed refresh.
function askModels(key: string, machine: string, agent: string, loaded: (() => void) | null) {
  let entry = MODELS.get(key);
  if (!entry) MODELS.set(key, entry = { list: undefined, at: 0, status: "idle", waiting: [] });
  if (loaded) entry.waiting.push(loaded);
  if (entry.status === "loading") return;
  entry.status = "loading";
  invoke<AgentModels | null>("agent_models", { machineId: machine, agent })
    .catch((error: unknown) => {
      logError(`Model lookup failed: machine=${machine} harness=${agent}: ${String(error)}`);
      return null;
    })
    .then(list => {
      const e = MODELS.get(key)!;
      const waiting = e.waiting;
      // A failed refresh keeps the list shown.
      const next = list ?? (e.list === undefined ? null : e.list);
      MODELS.set(key, { list: next, at: Date.now(), status: list ? "ready" : "failed", waiting: [] });
      if (list) storeModels();
      for (const tell of waiting) tell();
    });
}

/// Whether `e` holds an answer, which the pushes keep from then on.
const answered = (e: ModelsEntry | undefined) => e?.status === "ready" || e?.status === "failed";

/// The models `agent` offers on `machine` as known now: `undefined` only before
/// the first answer.
export function modelsOn(machine: string, agent: string, loaded: () => void): AgentModels | null | undefined {
  const key = `${machine}/${agent}`, known = MODELS.get(key);
  if (answered(known)) return known!.list;
  askModels(key, machine, agent, loaded);
  return known?.list;
}

/// The models `agent` offers on `machine`: the held list, else the answer to one read.
export function ensureModels(machine: string, agent: string): Promise<AgentModels | null> {
  return new Promise(resolve => {
    const list = modelsOn(machine, agent, () => resolve(knownModels(machine, agent) ?? null));
    if (list !== undefined) resolve(list);
  });
}

/// Takes the models a `machines` or `accounts` update pushes. A failed view keeps the list shown.
export function takeModels(data: unknown) {
  for (const { args, pushed } of pushedEach<AgentModels | null>(data, "agent_models")) {
    const { machineId, agent } = args;
    if (typeof machineId !== "string" || typeof agent !== "string") continue;
    const key = `${machineId}/${agent}`, e = MODELS.get(key), waiting = e?.waiting ?? [];

    if ("error" in pushed || !pushed.value) {
      if ("error" in pushed) logError(`Model lookup failed: machine=${machineId} harness=${agent}: ${pushed.error}`);
      MODELS.set(key, { list: e?.list ?? null, at: e?.at ?? 0, status: "failed", waiting: [] });
    } else {
      MODELS.set(key, { list: pushed.value, at: Date.now(), status: "ready", waiting: [] });
      storeModels();
    }

    for (const tell of waiting) tell();
  }
}

subscribeEach([topic("machines"), topic("accounts")], event => takeModels(event.data));

/// Reads the models of every harness on `hosts` ahead of the picker, such
/// as when a machine connects; lists that came are left alone, one that failed is asked again.
export function preloadModels(hosts: readonly { machine_id: string; agents: readonly string[] }[]) {
  for (const h of hosts) {
    for (const agent of h.agents) {
      const key = `${h.machine_id}/${agent}`;
      if (MODELS.get(key)?.status !== "ready") askModels(key, h.machine_id, agent, null);
    }
  }
}

/// The models known for `agent` on `machine`, without asking.
export const knownModels = (machine: string | null, agent: string | null) => machine && agent ? MODELS.get(`${machine}/${agent}`)?.list : undefined;
