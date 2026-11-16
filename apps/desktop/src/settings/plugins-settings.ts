// The Plugins page: what it holds, what its buttons do and the consent sheet.
// plugins-settings-state.ts has its words and flow; plugins-settings-view.tsx draws it.
import { createElement } from "react";
import { pickHere } from "../addproject/folderbrowser.tsx";
import { createStore } from "zustand/vanilla";
import { confirmSheet } from "../ui/menu.tsx";
import { openSheet } from "../ui/parts.tsx";
import { invoke, listen } from "../ui/tauri.ts";
import { ConsentView } from "./plugins-settings-view.tsx";
import {
  IDLE, UNEXPECTED, consentTitle, flowStep, openStage, parseFailure, parsePreview, parseRows, parseStaged, parseUpdateCheck,
  type Flow, type FlowEvent, type GitField, type PluginFailure, type PluginRow, type StagedPlugin, type UpdateState,
} from "./plugins-settings-state.ts";

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

  /// Reads the list again. A list that was shown stays when the read fails.
  async function load() {
    const mine = ++reads;
    try {
      const rows = parseRows(await call<unknown>("plugins_status"));
      if (mine !== reads) return;
      state.setState({ list: rows === null ? { kind: "failed", failure: UNEXPECTED } : { kind: "ready", rows } });
    } catch (error) {
      if (mine !== reads) return;
      state.setState(now => now.list.kind === "ready" ? now : { list: { kind: "failed", failure: parseFailure(error) } });
    }
  }

  /// Reads the list again whenever the desktop says a plugin changed. Returns what stops it.
  function follow() {
    let stopped = false, off: (() => void) | null = null;
    void hear("plugins-status", () => { void load(); }).then(stop => {
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

/// Asks before removing a plugin, naming it.
export function askRemove(store: PluginsSettings, row: PluginRow) {
  confirmSheet({
    title: `Remove ${row.name}?`,
    text: "Beep stops the plugin and deletes its folder from the plugins folder.",
    note: "You can install it again later.",
    action: "Remove",
    run: () => store.remove(row.id),
  });
}

/// Keeps the consent sheet open while the flow holds a consent, and draws it from the store.
export function showConsentSheets(store: PluginsSettings) {
  let sheet: ReturnType<typeof openSheet> | null = null;
  const draw = () => {
    const { flow } = store.state.getState();
    if (flow.kind !== "consent") {
      const open = sheet;
      sheet = null;
      open?.close();
      return;
    }

    sheet ??= openSheet("Plugin permissions", "am-sheet", {
      canClose: () => { const now = store.state.getState().flow; return now.kind !== "consent" || !now.busy; },
      onClose: () => { if (sheet !== null) { sheet = null; store.close(); } },
    });
    sheet.render(createElement(ConsentView, { flow, confirm: () => { void store.confirm(); }, cancel: store.close }), consentTitle(flow.consent.preview));
  };

  draw();
  const stop = store.state.subscribe(draw);

  return () => {
    stop();
    if (sheet !== null) store.close();
    const open = sheet;
    sheet = null;
    open?.close();
  };
}
