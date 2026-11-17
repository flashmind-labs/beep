// First run, Set up this computer: installs what this computer lacks of Git, Node.js, Claude Code
// and Codex, then opens each agent's sign-in in turn. The view follows `get` through `subscribe`.
import type { Machine } from "../machines/machines.tsx";
import { openSignIn } from "../machines/signin.tsx";
import { reason } from "../ui/format.ts";
import { subscribeChanges, topic } from "../ui/mesh-events.ts";
import { buildCapabilities, invoke } from "../ui/tauri.ts";
import { ITEMS, idleWork, isAgent, pending, row, type Item, type Setup, type Work } from "./firstrun-state.ts";

export interface SetupRun {
  get: () => Setup;
  subscribe: (listener: () => void) => () => void;
  start: () => void;
  /// Installs `item` when it is missing, then signs in to it when it is an agent.
  act: (item: Item) => void;
  skip: (item: Item) => void;
  dispose: () => void;
}

/// This computer as a machine of the mesh; null on a phone or before its node runs.
export async function thisComputer(): Promise<Machine | null> {
  if (!(await buildCapabilities()).local_host) return null;
  const [mac, list] = await Promise.all([
    invoke<{ id: string | null }>("mac_control").catch(e => { console.warn("this computer's state:", e); return null; }),
    invoke<Machine[]>("list_machines").catch(e => { console.warn("listing machines:", e); return []; }),
  ]);
  const id = mac?.id ?? null;
  return id === null ? null : list.find(m => m.id === id) ?? null;
}

export function setupRun(machine: Machine): SetupRun {
  let state: Setup = { env: machine.env, skipped: new Set(), installed: new Set(), work: idleWork(), running: false };
  let disposed = false;
  const listeners = new Set<() => void>();
  const update = (next: Partial<Setup>) => {
    state = { ...state, ...next };
    listeners.forEach(listener => listener());
  };
  const working = (item: Item, work: Work) => update({ work: { ...state.work, [item]: work } });
  const installed = (item: Item) => update({ installed: new Set([...state.installed, item]), work: { ...state.work, [item]: { kind: "idle" } } });

  // The node probes the machine again after each install and sign-in.
  const reread = async () => {
    const found = await thisComputer();
    if (found && !disposed) update({ env: found.env });
  };
  const unfollow = subscribeChanges(topic("machines"), { deliver: "latest", receive: reread });

  async function install(item: Item) {
    working(item, { kind: "installing" });

    try {
      if (isAgent(item)) {
        await invoke("agent_harness_update", { machineId: machine.id, agent: item });
        installed(item);
      } else {
        const done = await invoke<boolean>("machine_set_up", { machineId: machine.id, need: item });
        if (done) installed(item);
        else working(item, { kind: "dialog" });
      }
    } catch (e) {
      working(item, { kind: "failed", reason: reason(e) });
    }

    await reread();
  }

  const signIn = (item: Item) => new Promise<void>(done => {
    working(item, { kind: "signing_in" });
    openSignIn(machine, item, () => {
      working(item, { kind: "idle" });
      void reread().then(done);
    });
  });

  async function start() {
    if (state.running) return;
    update({ running: true });

    // Installs first, so each sign-in follows the other without a wait.
    for (const item of pending(state)) {
      if (disposed) return;
      const now = row(state, item).kind;
      if (now === "missing" || now === "failed") await install(item);
    }

    for (const item of ITEMS) {
      if (disposed) return;
      if (isAgent(item) && row(state, item).kind === "signed_out") await signIn(item);
    }

    update({ running: false });
  }

  async function one(item: Item) {
    if (state.running) return;
    update({ running: true });

    const now = row(state, item).kind;
    if (now === "missing" || now === "failed") await install(item);
    if (!disposed && isAgent(item) && row(state, item).kind === "signed_out") await signIn(item);

    update({ running: false });
  }

  return {
    get: () => state,
    subscribe: listener => { listeners.add(listener); return () => listeners.delete(listener); },
    start: () => { void start(); },
    act: item => { void one(item); },
    skip: item => update({ skipped: new Set([...state.skipped, item]) }),
    dispose: () => { disposed = true; unfollow(); },
  };
}
