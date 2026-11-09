import { invoke } from "../ui/tauri.ts";

type Invoke = typeof invoke;
interface SkillSyncState { enabled: boolean | null; busy: boolean; error: string }
export function createSkillSync(call: Invoke) {
  let state: SkillSyncState = { enabled: null, busy: false, error: "" };
  const listeners = new Set<() => void>();
  const update = (change: Partial<SkillSyncState>) => { state = { ...state, ...change }; for (const listener of listeners) listener(); };
  return {
    snapshot: () => state,
    subscribe(listener: () => void) { listeners.add(listener); return () => listeners.delete(listener); },
    async load() {
      if (state.busy) return;
      update({ enabled: null, busy: true, error: "" });
      try { update({ enabled: await call<boolean>("skill_sync_get") }); }
      catch (error) { update({ error: String(error) }); }
      finally { update({ busy: false }); }
    },
    async change() {
      if (state.busy || state.enabled === null) return;
      const enabled = !state.enabled;
      update({ busy: true, error: "" });
      try { await call("skill_sync_set", { enabled }); update({ enabled }); }
      catch (error) { update({ error: String(error) }); }
      finally { update({ busy: false }); }
    },
  };
}
export const skillSync = createSkillSync(invoke);
