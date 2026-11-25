import { lazy, Suspense, useEffect, useLayoutEffect, useMemo, useRef, type CSSProperties } from "react";
import { create } from "zustand";
import { addMention } from "../agents/workdrop.ts";
import { Button, ChipToggle } from "../ui/kit.tsx";
import { openMenu } from "../ui/menu.tsx";
import { onPhone, primaryModifier } from "../ui/platform.ts";
import { invoke } from "../ui/tauri.ts";
import { highlight, languageOf, MAX_LINES, shortSize } from "./files-state.ts";
import { hasPreview } from "./html-preview-state.ts";
import {
  formatLocationLabel,
  parseEditorSettings,
  parseLspResult,
  resolveVimSync,
  wordSpanAt,
} from "./lsp-state.ts";
import { type MediaTab, type TextPlace, type TreeText, discardSheet, newFileEdit, openLocation, P, rememberPane, saveWorkFile, openTabs } from "./media.ts";
import { noteTab, touchTab } from "./media-draw.ts";
import { fileError, readError } from "./read-file.ts";
import { readView, viewKey, writeView } from "./view-memory.ts";
import { mentioned } from "./worktree-state.ts";

const vimPreference = "beep.editor.vim";
const BUSY_NOTE = "Looking up definition…";

function readLocalVim(): string | null {
  try { return localStorage.getItem(vimPreference); } catch { return null; }
}

function writeLocalVim(enabled: boolean) {
  try { localStorage.setItem(vimPreference, String(enabled)); } catch { /* Keep it for this window. */ }
}

const useVimKeys = create<boolean>(() => readLocalVim() === "true");

window.addEventListener("storage", event => {
  if (event.key === vimPreference || event.key === null) useVimKeys.setState(event.newValue === "true");
});

let vimSyncState: "idle" | "loading" | "done" = "idle";
let vimTouched = false;

/// Hydrates Vim mode from mesh settings once per window so the choice persists
/// across restarts and machines, while keeping `localStorage` for instant boot.
function syncVimPreference() {
  if (vimSyncState !== "idle" || onPhone()) return;
  vimSyncState = "loading";

  void invoke<unknown>("editor_settings_get").then(raw => {
    vimSyncState = "done";
    if (vimTouched) return;
    const plan = resolveVimSync(readLocalVim(), parseEditorSettings(raw).vim);
    if (plan.writeLocal) writeLocalVim(plan.enabled);
    if (useVimKeys.getState() !== plan.enabled) useVimKeys.setState(plan.enabled);
    if (plan.writeRemote) void invoke("editor_settings_save", { settings: { vim: plan.enabled } }).catch(() => {});
  }).catch(() => { vimSyncState = "idle"; });
}

function setVimKeys(enabled: boolean) {
  vimTouched = true;
  writeLocalVim(enabled);
  useVimKeys.setState(enabled);
  if (!onPhone()) {
    void invoke("editor_settings_save", { settings: { vim: enabled } }).catch(() => {});
  }
}

let modTracking = false;

/// Toggles `.lsp-mod` on the document root while Cmd/Ctrl is held on desktop,
/// giving read-only code views a pointer cursor without per-tab listeners.
function trackDefinitionModifier() {
  if (modTracking || onPhone()) return;
  modTracking = true;
  const set = (on: boolean) => document.documentElement.classList.toggle("lsp-mod", on);
  const update = (event: KeyboardEvent) => set(primaryModifier(event));
  window.addEventListener("keydown", update);
  window.addEventListener("keyup", update);
  window.addEventListener("blur", () => set(false));
}

const CodeEditor = lazy(() => import("./code-editor.tsx"));
const SourceSearch = lazy(() => import("./code-editor.tsx").then(module => ({ default: module.SourceSearch })));

/// Shows an HTML or Markdown file's preview in its tab, reading it the first time, or its source again.
export async function togglePreview(tab: MediaTab) {
  const session = P.session;
  if (!session || !tab.rel) return;
  const preview = tab.preview ??= { active: false, loading: false, src: "", html: "", error: "" };
  tab.findRequest = 0;
  preview.active = !preview.active;
  rememberPane();
  if (!preview.active || preview.src || preview.html || preview.loading) { touchTab(tab); return; }
  preview.loading = true; preview.error = ""; touchTab(tab);

  try {
    Object.assign(preview, await readPreview(session, tab.root ?? session.cwd, tab.rel));
  } catch (error) {
    preview.error = fileError(error, tab.rel);
  } finally {
    preview.loading = false; touchTab(tab);
  }
}

/// Reads a tab's preview again after its file or one beside it changed,
/// keeping the one shown until the new one is read. One not shown is read
/// when it next shows.
export async function refreshPreview(tab: MediaTab) {
  const session = P.session, preview = tab.preview;
  if (!session || !tab.rel || !preview) return true;

  if (!preview.active) {
    Object.assign(preview, { src: "", html: "" });
    return true;
  }

  try {
    Object.assign(preview, await readPreview(session, tab.root ?? session.cwd, tab.rel), { error: "" });
    touchTab(tab);
    return true;
  } catch {
    // Keep the old stamp so the next check retries the preview.
    return false;
  }
}

/// An HTML file's page URL, fresh at each read, or a Markdown file's HTML.
async function readPreview(session: { machine_id: string }, root: string, path: string) {
  const result = await invoke<{ kind: string; src?: string; html?: string }>("agent_artifact", { machineId: session.machine_id, root, path, preview: true });
  if (result.kind === "html" && result.src) return { src: result.src, html: "" };
  if (result.kind === "md" && result.html !== undefined) return { src: "", html: result.html };
  throw new Error("This file cannot be previewed.");
}

/// Shows `now`, the file as it is now, in a text tab. An editor starts again
/// from it, or stops when the file can no longer be edited here.
export function takeText(tab: MediaTab, now: TreeText) {
  tab.text = now;
  tab.size = now.size;
  const edit = tab.edit;
  if (!edit) return;
  if (!now.editable) tab.edit = null;
  else Object.assign(edit, { base: now.text, view: null, state: null, generation: edit.generation + 1, dirty: false, conflict: false });
}

/// Starts editing a text tab's file, from the text it shows, with the cursor `at` a place.
function startEdit(tab: MediaTab, source: HTMLElement, at: TextPlace | null = null) {
  if (!tab.text?.editable) return;
  source.closest<HTMLElement>(".agpane")?.focus({ preventScroll: true });
  tab.findRequest = 0;
  tab.edit = newFileEdit(tab.text.text, at);
  touchTab(tab);
}

/// The 0-based line and column of a DOM node and offset inside the highlighted `<pre>`.
function placeInPre(pre: HTMLPreElement, node: Node, offset: number): TextPlace | null {
  if (!pre.contains(node)) return null;
  const range = document.createRange();
  range.setStart(pre, 0);
  range.setEnd(node, offset);
  const before = range.toString().split("\n"), line = before.length - 1;

  // Each line starts with its number.
  return { line, column: Math.max(0, (before.at(-1)?.length ?? 0) - String(line + 1).length) };
}

/// The place in the code view that the selection starts at, as a double click leaves it.
function selectedPlace(pre: HTMLPreElement): TextPlace | null {
  const selection = getSelection();
  if (!selection?.rangeCount || !selection.anchorNode) return null;
  return placeInPre(pre, selection.anchorNode, selection.anchorOffset);
}

interface CaretPosition { offsetNode: Node; offset: number }
type DocumentWithCaret = Document & {
  caretPositionFromPoint?: (x: number, y: number) => CaretPosition | null;
  caretRangeFromPoint?: (x: number, y: number) => Range | null;
};

/// The 0-based line and column under pointer coordinates `(clientX, clientY)` in `<pre>`.
function placeFromPoint(pre: HTMLPreElement, clientX: number, clientY: number): TextPlace | null {
  const doc = document as DocumentWithCaret;
  if (typeof doc.caretPositionFromPoint === "function") {
    const pos = doc.caretPositionFromPoint(clientX, clientY);
    if (pos) return placeInPre(pre, pos.offsetNode, pos.offset);
  }
  if (typeof doc.caretRangeFromPoint === "function") {
    const range = doc.caretRangeFromPoint(clientX, clientY);
    if (range) return placeInPre(pre, range.startContainer, range.startOffset);
  }
  return null;
}

/// Stops editing, asking first when there are unsaved changes.
function endEdit(tab: MediaTab) {
  const stop = () => { tab.findRequest = 0; tab.edit = null; tab.note = null; touchTab(tab); };
  if (tab.edit?.dirty) discardSheet(tab, stop); else stop();
}

/// Writes the editor's text to the file. Unless `overwrite`, it reads the file
/// first and stops when someone, such as the agent, changed it since.
async function saveEdit(tab: MediaTab, overwrite = false) {
  const s = P.session, edit = tab.edit;
  if (!s || !edit || !tab.rel || edit.saving) return;
  const at = { machineId: s.machine_id, root: tab.root ?? s.cwd, path: tab.rel };
  const text = (edit.view?.state ?? edit.state)?.sliceDoc() ?? edit.base;
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
    edit.dirty = (edit.view?.state ?? edit.state)?.sliceDoc() !== text;
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
    takeText(tab, await invoke<TreeText>("agent_tree_text", { machineId: s.machine_id, root: tab.root ?? s.cwd, path: tab.rel }));
    noteTab(tab, tab.edit ? `Reloaded ${tab.name}` : `${tab.name} can no longer be edited here.`);
  } catch (error) {
    noteTab(tab, readError(error));
  }
}

let definitionSeq = 0;
let busyTab: MediaTab | null = null;

function clearBusyNote() {
  if (!busyTab) return;
  const tab = busyTab;
  busyTab = null;
  if (tab.note === BUSY_NOTE) { tab.note = null; touchTab(tab); }
}

/// Asks the session's node for the definition of the symbol at 0-based `(line, column)`
/// and jumps directly when one target matches or opens a picker when several do.
async function lookupDefinition(
  tab: MediaTab,
  line: number,
  column: number,
  text: string | null,
  clientX: number,
  clientY: number,
  preferEdit: boolean,
) {
  const session = P.session;
  if (!session || !tab.rel) return;

  clearBusyNote();
  const root = tab.root ?? session.cwd;
  const seq = ++definitionSeq;
  const busyTimer = setTimeout(() => {
    if (definitionSeq !== seq || P.session !== session || !openTabs().includes(tab)) return;
    busyTab = tab;
    noteTab(tab, BUSY_NOTE);
  }, 250);

  try {
    const raw = await invoke<unknown>("agent_lsp_definition", {
      machineId: session.machine_id,
      root,
      path: tab.rel,
      line,
      column,
      text,
    });
    clearTimeout(busyTimer);
    if (definitionSeq !== seq || P.session !== session || !openTabs().includes(tab)) return;
    clearBusyNote();

    const result = parseLspResult(raw);
    if (result.locations.length === 1) {
      openLocation(root, result.locations[0], preferEdit);
      return;
    }

    if (result.locations.length > 1) {
      openMenu(
        { clientX, clientY },
        "Definitions",
        result.locations.map(loc => ({
          label: formatLocationLabel(loc),
          on: () => openLocation(root, loc, preferEdit),
        })),
      );
      return;
    }

    noteTab(tab, result.note ?? "No definition found.");
  } catch (error) {
    clearTimeout(busyTimer);
    if (definitionSeq !== seq || P.session !== session || !openTabs().includes(tab)) return;
    clearBusyNote();
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
  const findRequest = tab.findRequest ?? 0, finding = findRequest > 0;
  const foundLine = useRef<number | null>(null);
  const vimKeys = useVimKeys();
  const rel = tab.rel ?? "", lang = languageOf(rel);
  const scrollKey = P.session ? `${viewKey(P.session.machine_id, P.session.id)}:file:${tab.id}` : null;
  const { about, style, html, lines } = useMemo(() => {
    const all = text.binary ? [] : text.text.split("\n");
    if (all.length > 1 && all.at(-1) === "") all.pop();
    const count = all.length, shown = all.slice(0, MAX_LINES);
    const cut = text.cut || count > MAX_LINES ? ` · the first ${count > MAX_LINES ? `${MAX_LINES.toLocaleString()} lines` : shortSize(text.text.length)} shown` : "";
    const about = [rel, lang, text.binary ? "" : `${count.toLocaleString()} lines`, shortSize(text.size)].filter(Boolean).join(" · ") + cut;
    const style: CSSProperties & Record<"--lnw", string> = { "--lnw": `${String(shown.length).length}ch` };
    const html = shown.map((line, index) => `<span class="ln${index + 1 === tab.line ? " on" : ""}">${index + 1}</span>${highlight(line, lang)}`).join("\n");
    return { about, style, html, lines: shown };
  }, [text, rel, lang, tab.line]);
  const preview = tab.preview;
  const source = !preview?.active && !text.binary;
  const editable = text.editable && !onPhone();
  const edit = tab.edit;

  useEffect(() => {
    syncVimPreference();
    trackDefinitionModifier();
  }, []);

  const closeSearch = (line: number | null) => {
    foundLine.current = line;
    tab.findRequest = 0;
    touchTab(tab);
  };

  // The code scrolls back to where it was the last time the file showed.
  useLayoutEffect(() => {
    const saved = scrollKey && source ? readScrolled(scrollKey) : null;
    if (!code.current) return;
    if (foundLine.current !== null) {
      const line = pre.current?.querySelectorAll(".ln")[foundLine.current - 1];
      foundLine.current = null;
      if (line) { line.scrollIntoView({ block: "center" }); return; }
    }
    if (!saved) return;
    const frame = requestAnimationFrame(() => { if (code.current) { code.current.scrollTop = saved.top; code.current.scrollLeft = saved.left; } });
    return () => cancelAnimationFrame(frame);
  }, [scrollKey, source, finding]);

  // A link or definition jump to a line shows that line.
  useLayoutEffect(() => {
    if (!source || !tab.line) return;
    const frame = requestAnimationFrame(() => code.current?.querySelector(".ln.on")?.scrollIntoView({ block: "center" }));
    return () => cancelAnimationFrame(frame);
  }, [source, tab.line, tab.lineJump, html]);

  const scrolled = () => { if (scrollKey && code.current) writeView(scrollKey, { top: code.current.scrollTop, left: code.current.scrollLeft }); };

  if (edit) return <>
    <Suspense fallback={<div className="agpmsg">Loading the editor…</div>}>
      <CodeEditor
        key={edit.generation}
        edit={edit}
        rel={rel}
        vimKeys={vimKeys}
        findRequest={findRequest}
        closeSearch={() => closeSearch(null)}
        save={() => void saveEdit(tab)}
        changed={() => { if (!edit.dirty) { edit.dirty = true; tab.note = null; touchTab(tab); } }}
        goToDefinition={request => void lookupDefinition(tab, request.line, request.column, request.text, request.clientX, request.clientY, true)}
      />
    </Suspense>
    <div className="agpacts"><span className="meta mono" title={rel}>{tab.note ?? (edit.dirty ? `${rel} · edited` : rel)}</span><span className="sp" />
      <ChipToggle label="Vim" selected={vimKeys} onToggle={() => {
        setVimKeys(!vimKeys);
        edit.view?.focus();
      }} />
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
    : finding ? <Suspense fallback={<div className="agpmsg">Opening file search…</div>}>
      <SourceSearch text={text.text} findRequest={findRequest} close={closeSearch} />
    </Suspense>
    : <div
        className="agcode"
        style={style}
        ref={code}
        onScroll={scrolled}
        onClick={event => {
          if (onPhone() || !primaryModifier(event) || !pre.current) return;
          if (event.target instanceof Element && event.target.closest(".ln")) return;
          const place = placeFromPoint(pre.current, event.clientX, event.clientY);
          if (!place || !wordSpanAt(lines[place.line] ?? "", place.column)) return;
          event.preventDefault();
          void lookupDefinition(tab, place.line, place.column, null, event.clientX, event.clientY, false);
        }}
        onDoubleClick={event => {
          if (primaryModifier(event)) return;
          if (editable && pre.current) startEdit(tab, event.currentTarget, selectedPlace(pre.current));
        }}
      ><pre ref={pre} dangerouslySetInnerHTML={{ __html: html }} /></div>}
    <div className="agpacts"><span className="meta mono" title={rel}>{tab.note ?? about}</span><span className="sp" />
      {!text.binary && hasPreview(rel) && <Button data-preview="1" onClick={() => void togglePreview(tab)}>{preview?.active ? "Source" : "Preview"}</Button>}
      {editable && !preview?.active && <Button data-tedit="1" onClick={event => startEdit(tab, event.currentTarget)}>Edit</Button>}
      <Button data-tment="1" onClick={() => addMention(mentioned(tab.root ?? P.session?.cwd ?? "", P.session?.cwd ?? "", rel))}>Mention</Button>
      <Button data-tdownload="1" onClick={() => saveWorkFile(rel, text => noteTab(tab, text), tab.root)}>Download</Button>
    </div></>;
}
