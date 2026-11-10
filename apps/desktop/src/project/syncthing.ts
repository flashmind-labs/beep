import { chooseIgnoredFolders } from "./ignored-sync.tsx";
import type { IgnoredSync } from "./ignored-sync-state.ts";
// A project-wide Syncthing preference, applied by each machine.
import { createStore } from "zustand/vanilla";
import { invoke } from "../ui/tauri.ts";
import { confirmSheet, openMenu, type MenuItem } from "../ui/menu.ts";
import { reason } from "../ui/format.ts";
import { latest, STALE } from "../ui/active.ts";
import type { Project } from "./projects-state.ts";

interface SyncthingFiles {
  available: boolean;
  enabled: boolean;
  label: string | null;
}

/// An untouched import preserves the folder's existing Syncthing setting.
export function syncthingImportChoice() {
  let value: boolean | null = null;
  const state = createStore(() => ({ checked: true, disabled: false, indeterminate: true }));
  const lookup = latest();
  let lastTarget = "";
  const choice = {
    value: () => value,
    state,
    change: (checked: boolean) => { value = checked; state.setState({ checked, disabled: false, indeterminate: false }); },
    dispose: () => { lookup.cancel(); },
    follow: async (machine: string | null, folder: string | null) => {
      const target = `${machine ?? ""}\0${folder ?? ""}`;
      if (target === lastTarget) return;
      lastTarget = target;
      lookup.cancel();
      if (value !== null) return;
      state.setState({ checked: true, disabled: false, indeterminate: true });
      if (!machine || !folder) return;

      try {
        const files = await lookup.run(invoke<SyncthingFiles>("project_syncthing_files", { machineId: machine, folder }));
        if (files === STALE || value !== null) return;
        state.setState({ checked: files.enabled, disabled: !files.available, indeterminate: false });
      } catch { /* Import keeps the current setting if the machine cannot answer. */ }
    },
  };
  return choice;
}
export type SyncthingImportChoice = ReturnType<typeof syncthingImportChoice>;

/// The existing project menu holds the project-wide Syncthing toggle.
export function openProjectMenu(event: MouseEvent, project: Project, items: MenuItem[], onChanged: () => void) {
  const replace = openMenu(event, project.name, items);
  const extra = new Map<string, MenuItem>();
  const insert = (key: string, item: MenuItem) => {
    extra.set(key, item);
    replace([...items.slice(0, -1), ...extra.values(), ...items.slice(-1)]);
  };
  invoke<boolean>("project_git_auto_recovery", { project: project.name }).then(enabled => {
    const save = async () => {
      await invoke("project_git_auto_recovery", { project: project.name, enabled: !enabled });
      onChanged();
    };
    const change = async () => {
      if (!enabled) {
        confirmSheet({ title: "Preserve edits before Git sync?",
          text: "On all imported machines, Beep can stash staged, unstaged and untracked edits before updating an idle checkout to a newer commit, then restore those edits. Ignored files stay in place.",
          note: "Only fast forward updates are allowed. Local recovery stashes are kept. If restoration conflicts or is interrupted, sync stops for review and can offer an agent to help. Close sessions before recovery; external editors can still change files.",
          action: "Enable", run: save });
      } else {
        try { await save(); }
        catch (e) { openMenu(event, project.name, [{ label: `${reason(e)} (retry)`, on: change }]); }
      }
    };
    insert("recovery", { label: `Preserve edits before Git sync: ${enabled ? "On" : "Off"} (all machines)`, on: change });
  }, e => insert("recovery", { label: `Git recovery: ${reason(e)} (retry)`, on: () => openProjectMenu(event, project, items, onChanged) }));
  invoke<IgnoredSync>("project_ignored_sync", { project: project.name }).then(settings => {
    const change = async () => {
      if (!settings.enabled && !settings.folders.length) {
        chooseIgnoredFolders(project.name, { ...settings, enabled: true }, onChanged);
        return;
      }

      try {
        await invoke("project_ignored_sync", { project: project.name, settings: { ...settings, enabled: !settings.enabled } });
        onChanged();
      } catch (e) {
        openMenu(event, project.name, [{ label: `${reason(e)} (retry)`, on: change }]);
      }
    };
    insert("ignored", { label: `Sync gitignored: ${settings.enabled ? "On" : "Off"} (all machines)`, on: change });
    insert("ignored-folders", { label: "Choose ignored folders…", on: () => chooseIgnoredFolders(project.name, settings, onChanged) });
  }, e => insert("ignored", { label: `Sync gitignored: ${reason(e)} (retry)`, on: () => openProjectMenu(event, project, items, onChanged) }));
  invoke<boolean | null>("project_syncthing_mode", { project: project.name }).then(enabled => {
    const next = enabled !== false ? false : true;
    const change = async () => {
      try {
        await invoke("project_syncthing_mode", { project: project.name, enabled: next });
        onChanged();
      } catch (e) {
        openMenu(event, project.name, [{ label: `${reason(e)} (retry)`, on: change }]);
      }
    };
    insert("syncthing", { label: enabled === null ? "Use Beep sync (exclude from Syncthing)"
      : `Syncthing file sync: ${enabled ? "On" : "Off"} (all machines)`, on: change });
  }, e => insert("syncthing", { label: `Syncthing: ${reason(e)} (retry)`, on: () => openProjectMenu(event, project, items, onChanged) }));
}
