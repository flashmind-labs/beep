import { invoke } from "../ui/tauri.ts";

type Invoke = typeof invoke;
interface SyncSettingState { enabled: boolean | null; busy: boolean; error: string }
export function createSyncSetting(call: Invoke, kind: "skills" | "projects") {
  const get = kind === "skills" ? "skill_sync_get" : "project_sync_get";
  const set = kind === "skills" ? "skill_sync_set" : "project_sync_set";
  let state: SyncSettingState = { enabled: null, busy: false, error: "" };
  const listeners = new Set<() => void>();
  const update = (change: Partial<SyncSettingState>) => { state = { ...state, ...change }; for (const listener of listeners) listener(); };
  return {
    snapshot: () => state,
    subscribe(listener: () => void) { listeners.add(listener); return () => listeners.delete(listener); },
    async load() {
      if (state.busy) return;
      update({ enabled: null, busy: true, error: "" });
      try { update({ enabled: await call<boolean>(get) }); }
      catch (error) { update({ error: String(error) }); }
      finally { update({ busy: false }); }
    },
    async change() {
      if (state.busy || state.enabled === null) return;
      const enabled = !state.enabled;
      update({ busy: true, error: "" });
      try { await call(set, { enabled }); update({ enabled }); }
      catch (error) {
        update({ error: String(error) });
        if (kind === "projects") {
          // A failed mesh write may already have changed the master preference.
          try { update({ enabled: await call<boolean>(get) }); } catch { /* Keep the last confirmed value. */ }
        }
      }
      finally { update({ busy: false }); }
    },
  };
}
export const skillSync = createSyncSetting(invoke, "skills");
export const projectSync = createSyncSetting(invoke, "projects");
