import { chooseIgnoredFolders, editSyncRules } from "./ignored-sync.tsx";
import type { IgnoredSync } from "../../stores/project/ignored-sync-state.ts";
// A project-wide Syncthing preference, applied by each machine.
import { createStore } from "zustand/vanilla";
import { invoke } from "../../ui/tauri.ts";
import { confirmSheet, openMenu, type MenuAction, type MenuItem, type MenuLine } from "../../ui/menu.tsx";
import { reason } from "../../ui/format.ts";
import { latest, STALE } from "../../ui/active.ts";
import type { Project } from "../../stores/project/projects-state.ts";

export interface SyncthingFiles {
  available: boolean;
  enabled: boolean;
  label: string | null;
}

/// An untouched import preserves the folder's existing Syncthing setting.
/// The choice is `available`, so shown, only where Syncthing is in use.
export function syncthingImportChoice() {
  let value: boolean | null = null;
  const state = createStore(() => ({ checked: true, indeterminate: true, available: false, label: null as string | null }));
  const lookup = latest();
  let lastTarget = "";
  const choice = {
    value: () => value,
    state,
    change: (checked: boolean) => { value = checked; state.setState({ checked, indeterminate: false }); },
    dispose: () => { lookup.cancel(); },
    /// For a batch of folders: the choice shows when a machine has Syncthing folders.
    anywhere: async () => {
      const used = await invoke<boolean>("syncthing_in_use").catch(() => false);
      if (used) state.setState({ available: true });
    },
    follow: async (machine: string | null, folder: string | null) => {
      const target = `${machine ?? ""}\0${folder ?? ""}`;
      if (target === lastTarget) return;
      lastTarget = target;
      lookup.cancel();
      state.setState(value === null ? { checked: true, indeterminate: true, available: false, label: null } : { available: false, label: null });
      if (!machine || !folder) return;

      try {
        const found = await lookup.run(Promise.all([
          invoke<SyncthingFiles>("project_syncthing_files", { machineId: machine, folder }),
          invoke<boolean>("project_sync_get").catch(() => false),
        ]));
        if (found === STALE) return;

        // With project sync on, an untouched import leaves Syncthing.
        const [files, beep] = found;
        state.setState({ available: files.available, label: files.label });
        if (value === null) state.setState({ checked: files.enabled && !beep, indeterminate: false });
      } catch { /* Import keeps the current setting if the machine cannot answer. */ }
    },
  };
  return choice;
}
export type SyncthingImportChoice = ReturnType<typeof syncthingImportChoice>;

/// The project menu: `top` first, then the project-wide sync settings under
/// a label as each machine answers, then `remove`.
export function openProjectMenu(event: MouseEvent, project: Project, top: MenuItem[], remove: MenuAction, onChanged: () => void) {
  const sync = new Map<SyncLine, MenuLine[]>();
  const entries = (): MenuItem[] => {
    const lines = SYNC_LINES.flatMap(key => sync.get(key) ?? []);
    const section: MenuItem[] = lines.length ? [{ kind: "section", label: "Sync on all machines", items: lines }, { kind: "separator" }] : [];
    return [...top, { kind: "separator" }, ...section, remove];
  };
  const subtitle = project.machines.length ? `${project.machines.length} ${project.machines.length === 1 ? "machine" : "machines"}` : undefined;
  const replace = openMenu(event, project.name, entries(), subtitle);
  const reopen = () => openProjectMenu(event, project, top, remove, onChanged);
  const retry = (e: unknown, again: () => void) => openMenu(event, project.name, [{ label: `${reason(e)} (retry)`, on: again }]);
  const insert = (key: SyncLine, items: MenuLine[]) => {
    sync.set(key, items);
    replace(entries());
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
        catch (e) { retry(e, change); }
      }
    };
    insert("recovery", [{ kind: "toggle", label: "Preserve edits before Git sync", icon: "shield", checked: enabled, on: change }]);
  }, e => insert("recovery", [{ label: `Git recovery: ${reason(e)} (retry)`, icon: "shield", on: reopen }]));

  invoke<IgnoredSync>("project_ignored_sync", { project: project.name }).then(settings => {
    const change = async () => {
      if (!settings.enabled && !settings.folders.length) {
        chooseIgnoredFolders(project.name, project.machines, { ...settings, enabled: true }, onChanged);
        return;
      }

      try {
        await invoke("project_ignored_sync", { project: project.name, settings: { ...settings, enabled: !settings.enabled } });
        onChanged();
      } catch (e) {
        retry(e, change);
      }
    };
    const choose: MenuAction = {
      label: "Choose ignored folders",
      icon: "subArrow",
      sub: true,
      detail: settings.folders.length ? String(settings.folders.length) : undefined,
      chevron: true,
      on: () => chooseIgnoredFolders(project.name, project.machines, settings, onChanged),
    };
    insert("ignored", [{ kind: "toggle", label: "Sync ignored folders", icon: "folder", checked: settings.enabled, on: change }, ...(settings.enabled ? [choose] : [])]);
  }, e => insert("ignored", [{ label: `Ignored folders: ${reason(e)} (retry)`, icon: "folder", on: reopen }]));

  invoke<string>("project_sync_rules", { project: project.name }).then(rules => {
    insert("rules", [{ label: "Sync rules", icon: "sort", chevron: true, on: () => editSyncRules(project.name, rules, onChanged) }]);
  }, e => insert("rules", [{ label: `Sync rules: ${reason(e)} (retry)`, icon: "sort", on: reopen }]));

  // Unset keeps each folder's own Syncthing share, so it reads as Syncthing.
  // The line shows once set, or while some machine shares the folder.
  const shared = Promise.any(project.machines.map(place =>
    invoke<SyncthingFiles>("project_syncthing_files", { machineId: place.id, folder: place.folder })
      .then(files => files.available || Promise.reject(new Error("not shared")))));
  invoke<boolean | null>("project_syncthing_mode", { project: project.name }).then(async enabled => {
    if (enabled === null && !await shared.catch(() => false)) return;

    const beep = enabled === false;
    const change = async () => {
      try {
        await invoke("project_syncthing_mode", { project: project.name, enabled: beep });
        onChanged();
      } catch (e) {
        retry(e, change);
      }
    };
    insert("syncthing", [{ label: "Sync engine", icon: "sync", tag: beep ? "Beep" : "Syncthing", on: change }]);
  }, e => insert("syncthing", [{ label: `Syncthing: ${reason(e)} (retry)`, icon: "sync", on: reopen }]));
}

/// The sync settings, in the order the menu shows them.
const SYNC_LINES = ["recovery", "ignored", "rules", "syncthing"] as const;
type SyncLine = typeof SYNC_LINES[number];
