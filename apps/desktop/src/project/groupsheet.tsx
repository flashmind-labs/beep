import { useLayoutEffect, useRef } from "react";
import { flushSync } from "react-dom";
import { createDocumentView } from "../ui/document-root.tsx";
import { invoke } from "../ui/tauri.ts";
import { openSheet } from "../ui/parts.ts";
import { addGroupFolder, groupPath, type ProjectGroup } from "./projects-state.ts";
import { reason } from "../ui/format.ts";

export interface GroupSheetOptions {
  group?: ProjectGroup | null;
  matches: (paths: readonly string[]) => number;
  onSaved: (group: ProjectGroup) => void;
}
let root: HTMLElement | null = null;

export function openGroupSheet(options: GroupSheetOptions) {
  if (root) return;
  const group = options.group ?? null;
  let paths = [...(group?.paths ?? [])], error: string | null = null, saving = false;
  const shell = openSheet(group ? `Folders of ${group.name}` : "New group", "am-sheet", {
    canClose: () => !saving, onKey: keys,
    onClose: () => { root = null; view.unmount(); },
  });
  root = shell.root;
  const view = createDocumentView(shell.sheet);
  const field = () => shell.sheet.querySelector<HTMLInputElement>(".gp-field");
  const close = () => { if (!saving) shell.close(); };
  const add = () => {
    const input = field();
    if (!input) return false;
    const next = addGroupFolder(paths, input.value);
    paths = next.paths; error = next.error;
    if (!error) input.value = "";
    draw();
    input.focus();
    return !error;
  };
  const save = async () => {
    if (saving || (field()?.value.trim() && !add()) || !paths.length) return;
    saving = true;
    draw();
    try {
      const saved = await invoke<ProjectGroup>("project_group_save", { id: group?.id ?? null, name: group?.name ?? "", paths });
      saving = false;
      options.onSaved(saved);
      close();
    } catch (e) {
      saving = false; error = reason(e); draw();
      field()?.focus();
    }
  };
  function keys(e: KeyboardEvent) {
    if (e.key === "Escape") { e.preventDefault(); close(); }
    else if (e.key === "Enter" && e.metaKey) { e.preventDefault(); void save(); }
    else if (e.key === "Enter" && e.target === field()) { e.preventDefault(); if (field()?.value.trim()) add(); else void save(); }
    e.stopImmediatePropagation();
    return true;
  }
  function draw() {
    const n = options.matches(paths), named = group ? null : paths[0] || groupPath(field()?.value ?? "");
    const summary = paths.length ? `${named ? `Named ${named}. ` : ""}${n === 1 ? "1 project matches" : `${n} projects match`}.` : "Add a folder to make the group.";
    flushSync(() => view.render(<GroupSheet title={group?.name ?? "New group"} paths={paths} error={error} saving={saving} summary={summary}
      add={add} remove={path => { paths = paths.filter(p => p !== path); error = null; draw(); field()?.focus(); }} close={close} save={() => void save()} />));
  }
  draw();
}

function GroupSheet({ title, paths, error, saving, summary, add, remove, close, save }: { title: string; paths: readonly string[]; error: string | null; saving: boolean; summary: string; add(): boolean; remove(path: string): void; close(): void; save(): void }) {
  const input = useRef<HTMLInputElement>(null);
  useLayoutEffect(() => { input.current?.focus(); }, []);
  return <><div className="am-head"><b>{title}</b><kbd>esc</kbd></div><div className="am-label">Folder</div>
    <div className="am-line"><input ref={input} className="gp-field" placeholder="~/Projects/work" spellCheck={false} autoCapitalize="off" /><button type="button" className="dbtn" onClick={add}>Add</button></div>
    <div className="am-sub">Repos in this folder on any machine go in the group. ~/Projects/work and ~/work count as one folder.</div>
    {paths.map(path => <div className="gp-path" key={path}><span>{path}</span><button type="button" className="dbtn quiet warn small" onClick={() => remove(path)}>Remove</button></div>)}
    {error && <div className="am-msg bad">{error}</div>}<div className="am-sub">{summary}</div><div className="gp-foot"><button type="button" className="dbtn quiet" onClick={close}>Cancel</button><button type="button" className="dbtn pri" disabled={saving} onClick={save}>{saving ? "Saving…" : "Save group"}</button></div></>;
}
