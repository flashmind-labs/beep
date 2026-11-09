// flsh's own settings on its Coding agents page: OpenRouter routing and
// auto-compact, the same on every machine.
import type { invoke as Invoke } from "../ui/tauri.ts";
import { EMPTY, flshDraft, flshInput, flshProblem, type FlshDraft, type FlshSettings } from "./flsh-state.ts";

export interface FlshState {
  saved: FlshSettings | false | null;
  draft: FlshDraft | null;
  error: string;
  saving: boolean;
  requested: boolean;
}

export function flshPanel(invoke: typeof Invoke) {
  let state: FlshState = { saved: null, draft: null, error: "", saving: false, requested: false };
  const listeners = new Set<() => void>();
  const set = (change: Partial<FlshState>) => { state = { ...state, ...change }; for (const listener of listeners) listener(); };
  const fail = (e: unknown) => String(e).replace(/^(bad request|not found|error):\s*/i, "");

  async function load() {
    if (state.requested) return;
    set({ requested: true });
    try { set({ saved: (await invoke<FlshSettings | null>("flsh_settings_get")) ?? false, error: "" }); }
    catch (e) { set({ saved: EMPTY, error: fail(e) }); }
  }

  async function save() {
    const submitted = state.draft;
    if (!submitted || state.saving || flshProblem(submitted)) return;
    set({ saving: true, error: "" });
    try {
      await invoke("flsh_settings_save", { settings: flshInput(submitted) });
      const saved = (await invoke<FlshSettings | null>("flsh_settings_get")) ?? false;
      set({ saved, draft: state.draft === submitted ? null : state.draft, error: "" });
    } catch (e) { set({ error: fail(e) }); }
    finally { set({ saving: false }); }
  }

  function change(value: Partial<FlshDraft>) {
    if (!state.saved) return;
    set({ draft: { ...(state.draft ?? flshDraft(state.saved)), ...value } });
  }

  return {
    snapshot: () => state,
    subscribe: (listener: () => void) => { listeners.add(listener); return () => { listeners.delete(listener); }; },
    load, save, change,
    discard: () => set({ draft: null, error: "" }),
  };
}

export type FlshPanel = ReturnType<typeof flshPanel>;
