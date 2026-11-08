// The existing project menu opens this editor in the current window.
import { el } from "../ui/dom.ts";
import { dbtn, openSheet } from "../ui/parts.ts";
import { invoke } from "../ui/tauri.ts";
import { reason } from "../ui/format.ts";
import { ignoredSelection, type IgnoredSync } from "./ignored-sync-state.ts";

export function chooseIgnoredFolders(project: string, initial: IgnoredSync, onChanged: () => void) {
  let busy = false;
  const { sheet, close } = openSheet("Sync gitignored", "am-sheet", { canClose: () => !busy });
  const enabled = el("input");
  enabled.type = "checkbox";
  enabled.checked = initial.enabled;
  const paths = el("textarea", "ip-path");
  paths.rows = 4;
  paths.value = initial.folders.join("\n");
  paths.placeholder = "assets/models\ndata/captures";
  paths.setAttribute("aria-label", "Folders relative to the project, one per line");
  const error = el("div", "am-msg bad");
  error.setAttribute("role", "status");
  error.hidden = true;
  const save = dbtn("Save", "pri", async () => {
    if (busy) return;
    try {
      const settings = ignoredSelection(enabled.checked, paths.value);
      busy = true;
      save.disabled = enabled.disabled = paths.disabled = true;
      save.textContent = "Saving…";
      await invoke("project_ignored_sync", { project, settings });
      busy = false;
      close();
      onChanged();
    } catch (e) {
      busy = false;
      error.textContent = reason(e);
      error.hidden = false;
      save.disabled = enabled.disabled = paths.disabled = false;
      save.textContent = "Save";
    }
  });
  sheet.append(el("div", "am-head", el("b", "", "Sync gitignored"), el("kbd", "", "esc")),
    el("label", "am-line", enabled, "Sync selected ignored folders on all machines"),
    el("div", "am-label", "Folders relative to the project, one per line"), paths,
    el("div", "am-sub", "Each folder must be wholly ignored by Git and contain no tracked or staged files. These folders use file sync, including conflict copies. Other uncommitted edits stay local. Build output, linked repositories and paths excluded in .beepignore stay local."),
    el("div", "am-sub", "Beep waits while Syncthing manages these folders. Turning this off leaves local files in place."),
    error, el("div", "am-line ip-acts", dbtn("Cancel", "quiet", () => { if (!busy) close(); }), save));
  paths.focus();
}
