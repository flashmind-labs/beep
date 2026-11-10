import { createStore } from "zustand/vanilla";
import { invoke } from "../ui/tauri.ts";

type Invoke = typeof invoke;
interface SyncSettingState { enabled: boolean | null; busy: boolean; error: string }
export function createSyncSetting(call: Invoke, kind: "skills" | "projects") {
  const get = kind === "skills" ? "skill_sync_get" : "project_sync_get";
  const set = kind === "skills" ? "skill_sync_set" : "project_sync_set";
  const state = createStore<SyncSettingState>()(() => ({ enabled: null, busy: false, error: "" }));
  return {
    state,
    async load() {
      if (state.getState().busy) return;
      state.setState({ enabled: null, busy: true, error: "" });
      try { state.setState({ enabled: await call<boolean>(get) }); }
      catch (error) { state.setState({ error: String(error) }); }
      finally { state.setState({ busy: false }); }
    },
    async change() {
      const { busy, enabled: was } = state.getState();
      if (busy || was === null) return;
      const enabled = !was;
      state.setState({ busy: true, error: "" });
      try { await call(set, { enabled }); state.setState({ enabled }); }
      catch (error) {
        state.setState({ error: String(error) });
        if (kind === "projects") {
          // A failed mesh write may already have changed the master preference.
          try { state.setState({ enabled: await call<boolean>(get) }); } catch { /* Keep the last confirmed value. */ }
        }
      }
      finally { state.setState({ busy: false }); }
    },
  };
}
export const skillSync = createSyncSetting(invoke, "skills");
export const projectSync = createSyncSetting(invoke, "projects");
