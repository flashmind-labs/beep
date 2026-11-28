// flsh's own settings on its Coding agents page: OpenRouter routing and
// auto-compact, the same on every machine.
import { createStore } from "zustand";
import type { invoke as Invoke } from "../../ui/tauri.ts";
import { EMPTY, flshDraft, flshInput, flshProblem, type FlshDraft, type FlshSettings } from "./flsh-state.ts";
import { reason } from "../../ui/format.ts";

export interface FlshState {
  saved: FlshSettings | null;
  draft: FlshDraft | null;
  error: string;
  saving: boolean;
  requested: boolean;
}

export function flshPanel(invoke: typeof Invoke) {
  const state = createStore<FlshState>()(() => ({ saved: null, draft: null, error: "", saving: false, requested: false }));
  const set = state.setState;
  const fail = (e: unknown) => reason(e);

  async function load() {
    if (state.getState().requested) return;
    set({ requested: true });

    try { set({ saved: await invoke<FlshSettings>("flsh_settings_get"), error: "" }); }
    catch (e) { set({ saved: EMPTY, error: fail(e) }); }
  }

  async function save() {
    const { draft: submitted, saving } = state.getState();
    if (!submitted || saving || flshProblem(submitted)) return;
    set({ saving: true, error: "" });

    try {
      await invoke("flsh_settings_save", { settings: flshInput(submitted) });
      const saved = await invoke<FlshSettings>("flsh_settings_get");
      set(now => ({ saved, draft: now.draft === submitted ? null : now.draft, error: "" }));
    } catch (e) { set({ error: fail(e) }); }
    finally { set({ saving: false }); }
  }

  function change(value: Partial<FlshDraft>) {
    const { saved, draft } = state.getState();
    if (!saved) return;
    set({ draft: { ...(draft ?? flshDraft(saved)), ...value } });
  }

  return { state, load, save, change, discard: () => set({ draft: null, error: "" }) };
}

export type FlshPanel = ReturnType<typeof flshPanel>;
