// A project-wide Syncthing preference, applied by each machine.
import { invoke } from "../ui/tauri.ts";
import { el } from "../ui/dom.ts";
import { openMenu, type MenuItem } from "../ui/menu.ts";
import { reason } from "../ui/format.ts";
import type { Project } from "./projects-state.ts";

interface SyncthingFiles {
  available: boolean;
  enabled: boolean;
  label: string | null;
}

/// An untouched import preserves the folder's existing Syncthing setting.
export function syncthingImportChoice() {
  let value: boolean | null = null;
  const checkbox = el("input");
  checkbox.type = "checkbox";
  checkbox.checked = true;
  checkbox.indeterminate = true;
  checkbox.addEventListener("change", () => { value = checkbox.checked; });
  const node = el("div", "",
    el("label", "am-line", checkbox, "Keep Syncthing file sync"),
    el("div", "am-sub", "If Syncthing manages this folder, turn this off to use Beep on all machines. Git projects sync commits; uncommitted edits need a handoff. Other projects keep syncing."));
  let lookup = 0, lastTarget = "";
  async function follow(machine: string | null, folder: string | null) {
    const target = `${machine ?? ""}\0${folder ?? ""}`;
    if (target === lastTarget) return;
    lastTarget = target;
    const asked = ++lookup;
    if (value !== null) return;
    checkbox.disabled = false;
    checkbox.checked = true;
    checkbox.indeterminate = true;
    if (!machine || !folder) return;
    try {
      const files = await invoke<SyncthingFiles>("project_syncthing_files", { machineId: machine, folder });
      if (asked !== lookup || value !== null) return;
      checkbox.indeterminate = false;
      checkbox.checked = files.enabled;
      checkbox.disabled = !files.available;
    } catch { /* Import keeps the current setting if the machine cannot answer. */ }
  }
  return { node, value: () => value, follow };
}

/// The existing project menu holds the project-wide Syncthing toggle.
export function openProjectMenu(event: MouseEvent, project: Project, items: MenuItem[], onChanged: () => void) {
  const replace = openMenu(event, project.name, items);
  const insert = (item: MenuItem) => replace([...items.slice(0, -1), item, ...items.slice(-1)]);
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
    insert({ label: enabled === null ? "Use Beep sync (exclude from Syncthing)"
      : `Syncthing file sync: ${enabled ? "On" : "Off"} (all machines)`, on: change });
  }, e => insert({ label: `Syncthing: ${reason(e)} (retry)`, on: () => openProjectMenu(event, project, items, onChanged) }));
}
