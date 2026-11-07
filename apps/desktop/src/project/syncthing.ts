// Syncthing's per-checkout switch, shared by project import and settings.
import { invoke } from "../ui/tauri.ts";
import { button, el } from "../ui/dom.ts";
import { openSheet } from "../ui/parts.ts";
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
    el("label", "am-line", checkbox, "Use Syncthing file sync"),
    el("div", "am-sub", "Keep the current setting, or turn off to let Beep sync this project. Other projects in the shared folder keep syncing."));
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

export function openProjectSettings(project: Project, onChanged: () => void = () => {}) {
  let closed = false;
  const { sheet } = openSheet("Project settings", "am-sheet ip-sheet", { onClose: () => { closed = true; } });
  sheet.append(el("div", "am-head", el("b", "", "Project settings"), el("kbd", "", "esc")),
    el("div", "am-sub", "Syncthing file sync is set separately on each machine."));
  for (const place of project.machines) {
    const row = el("div", "", el("div", "am-label", place.name));
    const info = el("div", "am-sub", "Reading Syncthing settings...");
    const toggle = button("Syncthing file sync", "dbtn", () => save());
    toggle.setAttribute("role", "switch");
    toggle.disabled = true;
    row.append(toggle, info);
    sheet.append(row);
    let current: SyncthingFiles | null = null;
    const args = { machineId: place.id, folder: place.folder };
    function show(files: SyncthingFiles) {
      current = files;
      toggle.disabled = !files.available;
      toggle.setAttribute("aria-checked", String(files.enabled));
      toggle.textContent = `Syncthing file sync: ${files.enabled ? "On" : "Off"}`;
      info.textContent = !files.available ? "This folder is outside Syncthing's shared folders."
        : files.enabled ? "Syncthing syncs this project. Beep leaves it alone."
        : "Syncthing excludes this project. Beep can sync it; other projects keep syncing.";
    }
    async function save() {
      if (!current) return;
      toggle.disabled = true;
      info.textContent = "Saving...";
      try {
        const files = await invoke<SyncthingFiles>("project_syncthing_files", { ...args, enabled: !current.enabled });
        if (closed) return;
        const changed = files.enabled !== current.enabled;
        show(files);
        onChanged();
        if (!changed) info.textContent = "Another Syncthing ignore rule still excludes this project. Edit that rule in Syncthing to enable file sync.";
      } catch (e) {
        if (closed) return;
        info.textContent = reason(e);
        toggle.disabled = false;
      }
    }
    invoke<SyncthingFiles>("project_syncthing_files", args).then(files => {
      if (!closed) show(files);
    }, e => {
      if (!closed) {
        info.replaceChildren(el("span", "", reason(e)), button("Retry", "dbtn quiet", () => {
          invoke<SyncthingFiles>("project_syncthing_files", args).then(show, e => { info.textContent = reason(e); });
        }));
      }
    });
  }
}
