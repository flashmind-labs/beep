// The group sheet: the folders of a project group.
import { invoke } from "../ui/tauri.ts";
import { openSheet } from "../ui/parts.ts";
import { addGroupFolder, groupPath } from "./projects-state.ts";
import { button, el, present } from "../ui/dom.ts";
import type { ProjectGroup } from "./projects-state.ts";
import { reason } from "../ui/format.ts";

export interface GroupSheetOptions {
  /// The group to change; a new one without it.
  group?: ProjectGroup | null;
  /// How many projects a group of these folders holds now.
  matches: (paths: readonly string[]) => number;
  /// Runs once the server saved it.
  onSaved: (group: ProjectGroup) => void;
}

let root: HTMLElement | null = null;

export function openGroupSheet(options: GroupSheetOptions) {
  if (root) return;
  const group = options.group || null;
  let paths = [...(group?.paths || [])];
  let error: string | null = null;
  let saving = false;
  const shell = openSheet(group ? `Folders of ${group.name}` : "New group", "am-sheet", {
    canClose: () => !saving,
    onKey: keys,
    onClose: () => { root = null; },
  });
  root = shell.root;
  const sheet = shell.sheet;
  const field = el("input", "gp-field");
  field.placeholder = "~/Projects/work";
  field.spellcheck = false;
  field.autocapitalize = "off";

  const close = () => { if (!saving) shell.close(); };
  /// Adds what the field holds; false when it is refused.
  const add = () => {
    const next = addGroupFolder(paths, field.value);
    paths = next.paths;
    error = next.error;
    if (!error) field.value = "";
    draw();
    return !error;
  };
  const save = async () => {
    if (saving || (field.value.trim() && !add()) || !paths.length) return;
    saving = true;
    draw();
    try {
      const saved = await invoke<ProjectGroup>("project_group_save", { id: group?.id ?? null, name: group?.name ?? "", paths });
      saving = false;
      options.onSaved(saved);
      close();
    } catch (e) {
      saving = false;
      error = reason(e);
      draw();
    }
  };
  function keys(e: KeyboardEvent) {
    if (e.key === "Escape") { e.preventDefault(); close(); }
    else if (e.key === "Enter" && e.metaKey) { e.preventDefault(); save(); }
    else if (e.key === "Enter" && e.target === field) { e.preventDefault(); if (field.value.trim()) add(); else save(); }
    e.stopImmediatePropagation();
    return true;
  }

  function draw() {
    const rows = paths.map(p => el("div", "gp-path", el("span", "", p),
      button("Remove", "dbtn quiet warn small", () => { paths = paths.filter(x => x !== p); error = null; draw(); })));
    const n = options.matches(paths);
    const named = group ? null : paths[0] || groupPath(field.value);
    const sum = paths.length
      ? `${named ? `Named ${named}. ` : ""}${n === 1 ? "1 project matches" : `${n} projects match`}.`
      : "Add a folder to make the group.";
    const saveBtn = button(saving ? "Saving…" : "Save group", "dbtn pri", save);
    saveBtn.disabled = saving;
    sheet.replaceChildren(...present([
      el("div", "am-head", el("b", "", group ? group.name : "New group"), el("kbd", "", "esc")),
      el("div", "am-label", "Folder"),
      el("div", "am-line", field, button("Add", "dbtn", () => { add(); field.focus(); })),
      el("div", "am-sub", "Repos in this folder on any machine go in the group. ~/Projects/work and ~/work count as one folder."),
      ...rows,
      error ? el("div", "am-msg bad", error) : null,
      el("div", "am-sub", sum),
      el("div", "gp-foot", button("Cancel", "dbtn quiet", close), saveBtn),
    ]));
    // Drawing again moves the field, which loses the focus.
    if (!saving) field.focus();
  }

  draw();
}
