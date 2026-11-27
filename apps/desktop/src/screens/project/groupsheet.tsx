import { createRef, useLayoutEffect, type RefObject } from "react";
import { flushSync } from "react-dom";
import { invoke } from "../../ui/tauri.ts";
import { openSheet } from "../../ui/parts.tsx";
import { addGroupFolder, groupPath, type ProjectGroup } from "../../stores/project/projects-state.ts";
import { reason } from "../../ui/format.ts";
import { Button, NO_AUTO, SheetHead } from "../../ui/kit.tsx";

interface GroupSheetOptions {
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
    onClose: () => { root = null; },
  });
  root = shell.root;
  const input = createRef<HTMLInputElement>(), field = () => input.current;
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
    flushSync(() => shell.render(<GroupSheet input={input} title={group?.name ?? "New group"} paths={paths} error={error} saving={saving} summary={summary}
      add={add} remove={path => { paths = paths.filter(p => p !== path); error = null; draw(); field()?.focus(); }} close={close} save={() => void save()} />));
  }
  draw();
}

function GroupSheet({ input, title, paths, error, saving, summary, add, remove, close, save }: { input: RefObject<HTMLInputElement | null>; title: string; paths: readonly string[]; error: string | null; saving: boolean; summary: string; add(): boolean; remove(path: string): void; close(): void; save(): void }) {
  useLayoutEffect(() => { input.current?.focus(); }, []);
  return <><SheetHead title={title} /><div className="am-label">Folder</div>
    <div className="am-line"><input ref={input} className="gp-field" placeholder="~/Projects/work" {...NO_AUTO} /><Button onClick={add}>Add</Button></div>
    <div className="am-sub">Repos in this folder on any machine go in the group. ~/Projects/work and ~/work count as one folder.</div>
    {paths.map(path => <div className="gp-path" key={path}><span>{path}</span><Button look="quiet" danger small onClick={() => remove(path)}>Remove</Button></div>)}
    {error && <div className="am-msg bad">{error}</div>}<div className="am-sub">{summary}</div><div className="gp-foot"><Button look="quiet" onClick={close}>Cancel</Button><Button look="primary" disabled={saving} onClick={save}>{saving ? "Saving…" : "Save group"}</Button></div></>;
}
