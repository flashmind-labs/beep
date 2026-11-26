// The Plugins page: what it holds, what its buttons do and the consent sheet.
// plugins-settings-state.ts has its words and flow; plugins-settings-view.tsx draws it.
import { pickHere } from "../addproject/folderbrowser.tsx";
import { createStore } from "zustand/vanilla";
import { invoke, listen } from "../ui/tauri.ts";
import {
  IDLE, UNEXPECTED, flowStep, openStage, parseFailure, parsePreview, parseStaged, parseUpdateCheck,
  type Flow, type FlowEvent, type GitField, type PluginFailure, type StagedPlugin, type UpdateState,
} from "./plugins-settings-state.ts";
import { parseRows, type PluginRow } from "../ui/plugin-rows.ts";

type Call = typeof invoke;
type Listen = typeof listen;

export type ListState = { kind: "loading" } | { kind: "ready"; rows: PluginRow[] } | { kind: "failed"; failure: PluginFailure };

export interface PluginsState {
  list: ListState;
  flow: Flow;
  /// The plugins with a change under way.
  busy: readonly string[];
  /// What went wrong with each plugin's last change, by id.
  failures: ReadonlyMap<string, PluginFailure>;
  /// What each plugin's update check found, by id.
  updates: ReadonlyMap<string, UpdateState>;
}

const START: PluginsState = { list: { kind: "loading" }, flow: IDLE, busy: [], failures: new Map(), updates: new Map() };

export function createPluginsSettings(call: Call, hear: Listen) {
  const state = createStore<PluginsState>()(() => START);
  let reads = 0;

  const step = (event: FlowEvent) => state.setState(now => ({ flow: flowStep(now.flow, event) }));
  const flow = () => state.getState().flow;
  const without = <T>(map: ReadonlyMap<string, T>, id: string) => { const next = new Map(map); next.delete(id); return next; };
  const withEntry = <T>(map: ReadonlyMap<string, T>, id: string, value: T) => new Map(map).set(id, value);

  /// Reads the list. A list that was shown stays when the read fails.
  async function load() {
    const mine = ++reads;
    try {
      take(await call<unknown>("plugins_status"), mine);
    } catch (error) {
      if (mine !== reads) return;
      state.setState(now => now.list.kind === "ready" ? now : { list: { kind: "failed", failure: parseFailure(error) } });
    }
  }

  /// Shows `raw` as the list, unless a newer list or read came since `mine`.
  function take(raw: unknown, mine: number) {
    if (mine !== reads) return;
    const rows = parseRows(raw);
    state.setState({ list: rows === null ? { kind: "failed", failure: UNEXPECTED } : { kind: "ready", rows } });
  }

  /// Shows the list the desktop pushes whenever a plugin changed. Returns what stops it.
  function follow() {
    let stopped = false, off: (() => void) | null = null;
    // The payload is null while the desktop has not started its plugins: the first read has the list.
    void hear<unknown>("plugins-status", event => { if (event.payload !== null) take(event.payload, ++reads); }).then(stop => {
      if (stopped) stop(); else off = stop;
    }).catch(error => console.warn("plugins: no status events:", error));
    return () => { stopped = true; off?.(); };
  }

  /// Runs one change to a plugin, with its busy mark and its failure shown on its row.
  async function change(id: string, run: () => Promise<void>) {
    if (state.getState().busy.includes(id)) return;
    state.setState(now => ({ busy: [...now.busy, id], failures: without(now.failures, id) }));
    try {
      await run();
    } catch (error) {
      state.setState(now => ({ failures: withEntry(now.failures, id, parseFailure(error)) }));
    } finally {
      state.setState(now => ({ busy: now.busy.filter(other => other !== id) }));
    }
    await load();
  }

  const setEnabled = (id: string, enabled: boolean) => change(id, async () => { await call("plugins_enable", { id, enabled }); });

  function installFromGit() { step({ type: "open-git" }); }
  function editGit(field: GitField, value: string) { step({ type: "edit-git", field, value }); }

  async function submitGit() {
    step({ type: "submit-git" });
    const now = flow();
    if (now.kind !== "fetching") return;
    await stage(() => call<unknown>("plugins_stage_git", { repository: now.repository, reference: now.reference, folder: now.folder }));
  }

  /// Picks a folder on this computer and shows what installing it would do.
  async function installFromFolder() {
    if (flow().kind !== "idle") return;
    step({ type: "work" });
    try {
      const folder = await pickHere("Choose a plugin folder", null);
      if (folder === null) return step({ type: "closed" });
      accept(await call<unknown>("plugins_stage_folder", { folder }));
    } catch (error) {
      step({ type: "failed", failure: parseFailure(error) });
    }
  }

  /// Runs a call that stages a plugin and moves the flow to its consent, or its failure.
  async function stage(run: () => Promise<unknown>) {
    try {
      accept(await run());
    } catch (error) {
      step({ type: "failed", failure: parseFailure(error) });
    }
  }

  function accept(raw: unknown) {
    const staged: StagedPlugin | null = parseStaged(raw);
    if (staged === null) return step({ type: "failed", failure: UNEXPECTED });
    step({ type: "staged", staged });
  }

  /// Asks the plugin's repository for a newer commit, and stages it for consent when there is one.
  async function update(id: string) {
    if (flow().kind !== "idle" || state.getState().busy.includes(id)) return;
    state.setState(now => ({ updates: withEntry(now.updates, id, { kind: "checking" }), failures: without(now.failures, id) }));
    try {
      const check = parseUpdateCheck(await call<unknown>("plugins_check_update", { id }));
      if (check === null) throw UNEXPECTED;
      if (check.state !== "available") {
        state.setState(now => ({ updates: withEntry(now.updates, id, { kind: check.state }) }));
        return;
      }

      state.setState(now => ({ updates: without(now.updates, id) }));
      step({ type: "work" });
      try {
        accept(await call<unknown>("plugins_stage_update", { id }));
      } catch (error) {
        step({ type: "closed" });
        throw error;
      }
    } catch (error) {
      state.setState(now => ({ updates: withEntry(now.updates, id, { kind: "failed", message: parseFailure(error).message }) }));
    }
  }

  /// Shows what allowing a plugin that waits for approval would grant.
  async function allow(id: string) {
    if (flow().kind !== "idle") return;
    state.setState(now => ({ failures: without(now.failures, id) }));
    try {
      const preview = parsePreview(await call<unknown>("plugins_review", { id }));
      if (preview === null) throw UNEXPECTED;
      step({ type: "reviewed", preview });
    } catch (error) {
      state.setState(now => ({ failures: withEntry(now.failures, id, parseFailure(error)) }));
    }
  }

  /// The user agreed on the consent sheet: install, update or allow.
  async function confirm() {
    const now = flow();
    if (now.kind !== "consent" || now.busy) return;
    step({ type: "confirming" });
    try {
      if (now.consent.source === "staged") await call("plugins_confirm", { stage: now.consent.stage });
      else await call("plugins_allow", { id: now.consent.preview.id });
    } catch (error) {
      step({ type: "failed", failure: parseFailure(error) });
      return;
    }

    state.setState({ flow: IDLE });
    state.setState(current => ({ updates: without(current.updates, now.consent.preview.id) }));
    await load();
  }

  /// Ends the flow without installing; a staged plugin is thrown away.
  function close() {
    const now = flow();
    if (now.kind === "consent" && now.busy) return;
    const staged = openStage(now);
    step({ type: "closed" });
    if (staged !== null) void call("plugins_cancel", { stage: staged }).catch(error => console.warn("plugins: cancel failed:", error));
  }

  /// Deletes a plugin. A failure rejects with its message, which the confirm sheet shows.
  async function remove(id: string) {
    try {
      await call("plugins_remove", { id });
    } catch (error) {
      throw new Error(parseFailure(error).message);
    }
    await load();
  }

  async function openFolder() {
    try {
      await call("plugins_open_folder");
    } catch (error) {
      step({ type: "failed", failure: parseFailure(error) });
    }
  }

  return { state, load, follow, setEnabled, installFromGit, editGit, submitGit, installFromFolder, update, allow, confirm, close, remove, openFolder };
}

export type PluginsSettings = ReturnType<typeof createPluginsSettings>;

export const pluginsSettings = createPluginsSettings(invoke, listen);
