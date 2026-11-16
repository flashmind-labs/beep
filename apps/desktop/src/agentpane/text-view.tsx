import { lazy, Suspense, useLayoutEffect, useMemo, useRef, type CSSProperties } from "react";
import { addMention } from "../agents/workdrop.ts";
import { invoke } from "../ui/tauri.ts";
import { highlight, languageOf, MAX_LINES, shortSize } from "./files-state.ts";
import { hasPreview } from "./html-preview-state.ts";
import { type MediaTab, type TextPlace, type TreeText, discardSheet, P, saveWorkFile } from "./media.ts";
import { noteTab, touchTab } from "./media-draw.ts";
import { fileError, readError } from "./read.ts";
import { readView, viewKey, writeView } from "./view-memory.ts";
import { mentioned } from "./worktree-state.ts";
import { Button } from "../ui/kit.tsx";
import { onPhone } from "../ui/platform.ts";

const CodeEditor = lazy(() => import("./code-editor.tsx"));

/// Shows an HTML or Markdown file's preview in its tab, reading it the first time, or its source again.
export async function togglePreview(tab: MediaTab) {
  const session = P.session;
  if (!session || !tab.rel) return;
  const preview = tab.preview ??= { active: false, loading: false, src: "", html: "", error: "" };
  preview.active = !preview.active;
  if (!preview.active || preview.src || preview.html || preview.loading) { touchTab(tab); return; }
  preview.loading = true; preview.error = ""; touchTab(tab);

  try {
    const result = await invoke<{ kind: string; src?: string; html?: string }>("agent_artifact", { machineId: session.machine_id, root: tab.root ?? session.cwd, path: tab.rel, preview: true });
    if (result.kind === "html" && result.src) preview.src = result.src;
    else if (result.kind === "md" && result.html !== undefined) preview.html = result.html;
    else throw new Error("This file cannot be previewed.");
  } catch (error) {
    preview.error = fileError(error, tab.rel);
  } finally {
    preview.loading = false; touchTab(tab);
  }
}

/// Starts editing a text tab's file, from the text it shows, with the cursor `at` a place.
function startEdit(tab: MediaTab, at: TextPlace | null = null) {
  if (!tab.text?.editable) return;
  tab.edit = { base: tab.text.text, view: null, state: null, generation: 0, dirty: false, saving: false, conflict: false, at };
  touchTab(tab);
}

/// The place in the code view that the selection starts at, as a double click leaves it.
function selectedPlace(pre: HTMLPreElement): TextPlace | null {
  const selection = getSelection();
  if (!selection?.rangeCount || !pre.contains(selection.anchorNode)) return null;
  const range = document.createRange();
  range.setStart(pre, 0);
  range.setEnd(selection.anchorNode ?? pre, selection.anchorOffset);
  const before = range.toString().split("\n"), line = before.length - 1;

  // Each line starts with its number.
  return { line, column: Math.max(0, (before.at(-1)?.length ?? 0) - String(line + 1).length) };
}

/// Stops editing, asking first when there are unsaved changes.
function endEdit(tab: MediaTab) {
  const stop = () => { tab.edit = null; tab.note = null; touchTab(tab); };
  if (tab.edit?.dirty) discardSheet(tab, stop); else stop();
}

/// Writes the editor's text to the file. Unless `overwrite`, it reads the file
/// first and stops when someone, such as the agent, changed it since.
async function saveEdit(tab: MediaTab, overwrite = false) {
  const s = P.session, edit = tab.edit;
  if (!s || !edit || !tab.rel || edit.saving) return;
  const at = { machineId: s.machine_id, root: tab.root ?? s.cwd, path: tab.rel };
  const text = (edit.view?.state ?? edit.state)?.doc.toString() ?? edit.base;
  edit.saving = true;
  noteTab(tab, `Saving ${tab.name}…`);

  try {
    if (!overwrite) {
      const now = await invoke<TreeText>("agent_tree_text", at);
      if (!now.editable || now.text !== edit.base) {
        edit.conflict = true;
        noteTab(tab, `${tab.name} changed since you opened it.`);
        return;
      }
    }

    await invoke("agent_tree_edit", { machineId: at.machineId, root: at.root, action: { Write: { path: at.path, text } } });
    edit.base = text;
    edit.conflict = false;
    edit.dirty = (edit.view?.state ?? edit.state)?.doc.toString() !== text;
    tab.text = { size: new TextEncoder().encode(text).length, text, cut: false, binary: false, editable: true };
    noteTab(tab, `Saved ${tab.name}`);
  } catch (error) {
    noteTab(tab, readError(error));
  } finally {
    edit.saving = false;
    touchTab(tab);
  }
}

/// Drops the edits and starts again from the file as it is now.
async function reloadEdit(tab: MediaTab) {
  const s = P.session, edit = tab.edit;
  if (!s || !edit || !tab.rel) return;

  try {
    const now = await invoke<TreeText>("agent_tree_text", { machineId: s.machine_id, root: tab.root ?? s.cwd, path: tab.rel });
    tab.text = now;
    if (!now.editable) { tab.edit = null; noteTab(tab, `${tab.name} can no longer be edited here.`); return; }
    Object.assign(edit, { base: now.text, view: null, state: null, generation: edit.generation + 1, dirty: false, conflict: false });
    noteTab(tab, `Reloaded ${tab.name}`);
  } catch (error) {
    noteTab(tab, readError(error));
  }
}

/// Where the code of a file's tab was scrolled to, kept with the session's views.
interface Scrolled { top: number; left: number }

const readScrolled = (key: string): Scrolled | null => {
  const saved = readView(key);
  return saved && typeof saved === "object" && "top" in saved && "left" in saved && typeof saved.top === "number" && typeof saved.left === "number" ? { top: saved.top, left: saved.left } : null;
};

export function TextView({ tab, text }: { tab: MediaTab; text: TreeText }) {
  const code = useRef<HTMLDivElement>(null), pre = useRef<HTMLPreElement>(null);
  const rel = tab.rel ?? "", lang = languageOf(rel);
  const scrollKey = P.session ? `${viewKey(P.session.machine_id, P.session.id)}:file:${tab.id}` : null;
  const { about, style, html } = useMemo(() => {
    const all = text.binary ? [] : text.text.split("\n");
    if (all.length > 1 && all.at(-1) === "") all.pop();
    const lines = all.length, shown = all.slice(0, MAX_LINES);
    const cut = text.cut || lines > MAX_LINES ? ` · the first ${lines > MAX_LINES ? `${MAX_LINES.toLocaleString()} lines` : shortSize(text.text.length)} shown` : "";
    const about = [rel, lang, text.binary ? "" : `${lines.toLocaleString()} lines`, shortSize(text.size)].filter(Boolean).join(" · ") + cut;
    const style: CSSProperties & Record<"--lnw", string> = { "--lnw": `${String(shown.length).length}ch` };
    const html = shown.map((line, index) => `<span class="ln${index + 1 === tab.line ? " on" : ""}">${index + 1}</span>${highlight(line, lang)}`).join("\n");
    return { about, style, html };
  }, [text, rel, lang, tab.line]);
  const preview = tab.preview;
  const source = !preview?.active && !text.binary;
  const editable = text.editable && !onPhone();

  // The code scrolls back to where it was the last time the file showed.
  useLayoutEffect(() => {
    const saved = scrollKey && source ? readScrolled(scrollKey) : null;
    if (!saved) return;
    const frame = requestAnimationFrame(() => { if (code.current) { code.current.scrollTop = saved.top; code.current.scrollLeft = saved.left; } });
    return () => cancelAnimationFrame(frame);
  }, [scrollKey, source]);

  // A link to a line shows that line.
  useLayoutEffect(() => {
    if (!source || !tab.line) return;
    const frame = requestAnimationFrame(() => code.current?.querySelector(".ln.on")?.scrollIntoView({ block: "center" }));
    return () => cancelAnimationFrame(frame);
  }, [source, tab.line, html]);

  const scrolled = () => { if (scrollKey && code.current) writeView(scrollKey, { top: code.current.scrollTop, left: code.current.scrollLeft }); };
  const edit = tab.edit;
  if (edit) return <>
    <Suspense fallback={<div className="agpmsg">Loading the editor…</div>}>
      <CodeEditor key={edit.generation} edit={edit} rel={rel} save={() => void saveEdit(tab)} changed={() => { if (!edit.dirty) { edit.dirty = true; tab.note = null; touchTab(tab); } }} />
    </Suspense>
    <div className="agpacts"><span className="meta mono" title={rel}>{tab.note ?? (edit.dirty ? `${rel} · edited` : rel)}</span><span className="sp" />
      {edit.conflict ? <>
        <Button data-treload="1" onClick={() => void reloadEdit(tab)}>Reload</Button>
        <Button data-toverwrite="1" look="primary" disabled={edit.saving} onClick={() => void saveEdit(tab, true)}>Overwrite</Button>
      </> : <Button data-tsave="1" look="primary" disabled={edit.saving || !edit.dirty} onClick={() => void saveEdit(tab)}>Save</Button>}
      <Button data-tdone="1" onClick={() => endEdit(tab)}>Done</Button>
    </div></>;

  return <>{preview?.active ? preview.loading ? <div className="agpmsg">Loading preview…</div>
    : preview.error ? <div className="agpmsg derr">{preview.error}</div>
    : preview.html ? <div className="agartmd" dangerouslySetInnerHTML={{ __html: preview.html }} />
    : <div className="agartframe-box"><iframe className="agartframe" sandbox="allow-scripts" referrerPolicy="no-referrer" title={`Preview of ${tab.name}`} src={preview.src || undefined} /></div>
    : text.binary ? <div className="agpmsg">{tab.name} is not text ({shortSize(text.size)}). Save it to open it on this computer.</div>
    : <div className="agcode" style={style} ref={code} onScroll={scrolled} onDoubleClick={() => { if (editable && pre.current) startEdit(tab, selectedPlace(pre.current)); }}><pre ref={pre} dangerouslySetInnerHTML={{ __html: html }} /></div>}
    <div className="agpacts"><span className="meta mono" title={rel}>{tab.note ?? about}</span><span className="sp" />
      {!text.binary && hasPreview(rel) && <Button data-preview="1" onClick={() => void togglePreview(tab)}>{preview?.active ? "Source" : "Preview"}</Button>}
      {editable && !preview?.active && <Button data-tedit="1" onClick={() => startEdit(tab)}>Edit</Button>}
      <Button data-tment="1" onClick={() => addMention(mentioned(tab.root ?? P.session?.cwd ?? "", P.session?.cwd ?? "", rel))}>Mention</Button>
      <Button data-tdownload="1" onClick={() => saveWorkFile(rel, text => noteTab(tab, text), tab.root)}>Download</Button>
    </div></>;
}
