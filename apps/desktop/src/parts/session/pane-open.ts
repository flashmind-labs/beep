// Opening and showing the pane's tabs: the file, picture and folder tabs, and the Files tab. The
// tabs call these directly; media.ts, which lays the docks out, draws what they change.
import { hasPreview } from "../../stores/agentpane/html-preview-state.ts";
import { FILES, baseName, type Dock, mediaKind, resolvePath, type FileRef } from "../../stores/agentpane/media-state.ts";
import { paneRoot, checkoutTop } from "../../stores/agentpane/worktree.ts";
import type { LspLocation } from "../../stores/agentpane/lsp-state.ts";
import { load, loadText, startTerminal, togglePreview } from "./read.ts";
import { dockOf, desktopChatPane, openTabs, paneModel, paneOf, setHidden, setOn } from "./media-layout.ts";
import { rememberPane, saveLayout } from "./pane-memory.ts";
import { P, newFileEdit, redraw, type MediaTab, type WorkFile } from "../../stores/agentpane/pane-state.ts";
import { onPhone } from "../../ui/platform.ts";

export function addTab(tab: MediaTab) {
  paneModel.setState(s => ({ tabs: [...s.tabs, tab] }));
}

/// Folds dock `d` or shows it, as the user asked. A right pane shown on a window too narrow for
/// it beside the conversation stays shown once the window is wide, as when an assistant chat's
/// list steps aside for it; one hidden there leaves the wide layout's choice as it was.
export function setFolded(d: Dock, folded: boolean) {
  setHidden(d, folded);
  if (d === "right" && folded) P.rightRequested = false;
  if (d === "right" && !folded && paneModel.getState().wideFolded !== null) paneModel.setState({ wideFolded: false });
}

/// On a phone, which shows one pane at a time, a tab being shown takes the screen from the log.
export function leaveLog() {
  if (paneModel.getState().phone) paneModel.setState({ log: false });
}

/// Shows tab `id`, unfolding its dock, and gives the pane the keys.
export function focusTab(id: string) {
  const d = dockOf(id);
  leaveLog();
  setFolded(d, false);
  saveLayout();
  setOn(d, id);
  rememberPane();
  redraw();
  paneOf(d)?.focus({ preventScroll: true });
}

export function open(t: Pick<MediaTab, "id" | "kind" | "name" | "path" | "url"> & Partial<MediaTab>) {
  if (!openTabs().some(x => x.id === t.id)) {
    const tab: MediaTab = { size: 0, started: Date.now(), done: 0, error: "", rev: 0, viewHidden: false, note: null, player: null, playerAt: 0, ...t };
    addTab(tab);

    if (!tab.error) {
      if (tab.kind === "text") loadText(tab);
      else if (tab.kind === "term") startTerminal(tab);
      else if (!tab.url && !tab.exported) load(tab);
    }
  }

  focusTab(t.id);
}

/// Opens the file at `path` (relative to the session's folder, absolute, or
/// under `~/`) on the session's machine, when it is one the pane shows.
export function openPath(path: string) {
  const kind = mediaKind(path);
  if (!kind || !P.session) return false;
  const full = resolvePath(P.session.cwd, path);
  open({ id: full, kind, name: baseName(full), path: full, url: null });
  return true;
}

/// Opens a picture the log already shows (a data URL) in its own tab.
export function openPicture(url: string, name: string) {
  open({ id: url, kind: "image", name: baseName(name), path: null, url });
}

/// Opens `rel`, a file in the session's folder, from the Files tab: a
/// picture, video, audio, 3D model or PDF in its viewer, anything else as text.
export function openWorkFile(rel: string) {
  openIn(paneRoot(), rel);
}

/// Opens an image, HTML or Markdown file directly in its preview.
export function previewWorkFile(rel: string) {
  const tab = openIn(paneRoot(), rel);
  if (tab) previewTab(tab);
}

export function previewTab(t: MediaTab) {
  if (t.kind !== "text" || t.preview?.active) return;
  if (!t.text) {
    t.previewQueued = true;
    rememberPane();
    return;
  }
  void togglePreview(t);
}

/// Opens `rel`, a file in the worktree at `root`, and gives its tab;
/// `elsewhere` is where else the file may be when it is not there.
export function openIn(root: string, rel: string, elsewhere: WorkFile[] = []) {
  const full = rel.startsWith("/") ? rel : resolvePath(root, rel);
  const kind = mediaKind(full);
  // A file of the session's own folder keeps its tab through a switch.
  const id = kind ? full : root === P.session?.cwd.replace(/\/+$/, "") ? `text:${rel}` : `text:${root}\n${rel}`;

  if (kind) open({ id, kind, name: baseName(full), path: full, url: null, elsewhere });
  else open({ id, kind: "text", name: baseName(rel), path: null, url: null, rel, root, elsewhere });

  return openTabs().find(t => t.id === id) ?? null;
}

/// Opens the file a link in a reply points at, at its line. A relative
/// path is read from the worktree the pane shows, else from the session's
/// folder, else from the top of its checkout, where a session in a
/// subfolder finds the repository's worktrees. One outside the session's
/// folder opens as a tab saying the node would not read it.
export function openLinked(ref: FileRef) {
  const s = P.session;
  if (!s) return;

  if (ref.path.startsWith("~")) {
    openPath(ref.path);
    return;
  }

  const cwd = s.cwd.replace(/\/+$/, "");
  const shown = (paneRoot() || cwd).replace(/\/+$/, "");
  const full = resolvePath(shown, ref.path);
  const elsewhere = ref.path.startsWith("/") ? [] : [cwd, checkoutTop()]
    .filter((other): other is string => other !== null && other !== shown)
    .map(other => ({ root: other, full: resolvePath(other, ref.path) }))
    .filter(({ root, full }) => full.startsWith(`${root}/`))
    .map(({ root, full }) => ({ root, rel: full.slice(root.length + 1) }));
  const root = [shown, cwd].find(root => full.startsWith(`${root}/`)) ?? cwd;
  const rel = full.startsWith(`${root}/`) ? full.slice(root.length + 1) : full;
  // A tab that found the file at one of these places already, such as one
  // restored under the folder it was found in, is the one to show.
  const places = [{ root, rel }, ...elsewhere].map(place => resolvePath(place.root, place.rel));
  const held = openTabs().find(t => !t.closed && t.kind !== "term" && places.includes(t.rel ? resolvePath(t.root ?? cwd, t.rel) : t.path ?? ""));
  if (held) focusTab(held.id);
  const tab = held ?? openIn(root, rel, elsewhere);

  // An HTML page or Markdown file shows rendered; Source switches to its code.
  if (tab?.kind === "text" && tab.rel && hasPreview(tab.rel) && !tab.preview) previewTab(tab);

  if (tab && (tab.line !== ref.line || ref.line !== null)) {
    tab.line = ref.line;
    if (ref.line !== null) tab.lineJump = (tab.lineJump ?? 0) + 1;
    tab.rev++;
    redraw();
  }
}

/// Opens a definition target returned by LSP at its 0-based line and column.
/// When `preferEdit` is set, opens or focuses the target in CodeMirror edit mode
/// so Cmd+click and Vim `gd` keep the editing flow intact.
export function openLocation(root: string, location: LspLocation, preferEdit: boolean) {
  const s = P.session;
  if (!s) return;

  if (!location.inside_root) {
    openLinked({ path: location.path, line: location.line + 1 });
    return;
  }

  const cwd = s.cwd.replace(/\/+$/, "");
  const cleanRoot = root.replace(/\/+$/, "");
  const targetFull = resolvePath(cleanRoot, location.path);
  const held = openTabs().find(t => !t.closed && t.kind === "text" && t.rel && resolvePath(t.root ?? cwd, t.rel) === targetFull);
  const tab = held ?? openIn(cleanRoot, location.path);
  if (!tab || tab.kind !== "text") return;

  tab.findRequest = 0;
  if (tab.preview) tab.preview.active = false;
  tab.line = location.line + 1;
  tab.lineJump = (tab.lineJump ?? 0) + 1;

  const at = { line: location.line, column: location.column };
  if (tab.edit) {
    tab.edit.at = at;
  } else if (preferEdit && !onPhone()) {
    if (tab.text?.editable) tab.edit = newFileEdit(tab.text.text, at);
    else if (!tab.text) tab.editAt = at;
  }

  tab.rev++;
  if (held) focusTab(held.id); else redraw();
}

export function requestRightPane(d: Dock) {
  const { split, places } = paneModel.getState();
  if (d === "right" && split && !places.right.shown && !places.right.strip && desktopChatPane()) P.rightRequested = true;
}

/// Shows the Files tab, unfolding its dock.
export function showFiles() {
  const d = dockOf(FILES);
  leaveLog();
  requestRightPane(d);
  setOn(d, FILES);
  setFolded(d, false);
  redraw();
}
