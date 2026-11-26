// What the pane remembers of a session: its tabs, docks and layout, kept for
// the next time the session shows. Below the tabs, which ask it to save.
import { readWindowPreference, writeWindowPreference, windowPreferenceKey } from "../ui/window-layout.ts";
import type { TerminalRow } from "../terminals/terminals.tsx";
import { readLayout, withFolded, type PaneLayout } from "./media-state.ts";
import { paneModel, openTabs } from "./media-layout.ts";
import { P, type MediaTab } from "./pane-state.ts";
import { PANE } from "../bar/core.ts";
import { readView, viewKey, writeView } from "./view-memory.ts";

const KEY = `beep.mediaPane.${PANE ? "window" : "bar"}`;

/// The pane is released: its views went and what it remembers stays as it was.
export const memory = { released: false };

export function savedPane(key: string): unknown {
  const scoped = windowPreferenceKey(key);
  return readView(scoped) ?? (scoped === key ? null : readView(key));
}

/// Terminals still starting, by session.
export const startingTerminals = new Map<string, MediaTab[]>();
export function rememberPane() {
  if (memory.released || !P.session) return;
  const key = viewKey(P.session.machine_id, P.session.id);
  const pending = openTabs().filter(t => t.kind === "term" && !t.term && !t.existing && !t.closed);
  if (pending.length) startingTerminals.set(key, pending); else startingTerminals.delete(key);
  writeView(windowPreferenceKey(key), { on: paneModel.getState().on, layout: keptLayout(), zoom: paneModel.getState().zoom, tabs: openTabs().filter(t => !t.closed && !t.exported).map(savedTab) });
}

export function savedTab(t: MediaTab) {
  if (t.kind === "term") return { id: t.id, name: t.name, terminal: t.term?.row ?? t.existing ?? null };
  if (t.rel && t.root) return { root: t.root, rel: t.rel, preview: t.previewQueued === true || t.preview?.active === true };
  return { path: t.path, url: t.path ? null : t.url, name: t.name };
}

export function readTerminal(value: unknown): TerminalRow | null {
  if (!value || typeof value !== "object") return null;
  if (!("id" in value && typeof value.id === "string"
    && "project" in value && typeof value.project === "string"
    && "machine_id" in value && typeof value.machine_id === "string"
    && "machine_name" in value && typeof value.machine_name === "string"
    && "name" in value && typeof value.name === "string"
    && "command" in value && typeof value.command === "string"
    && "cwd" in value && typeof value.cwd === "string"
    && "state" in value && typeof value.state === "string"
    && "code" in value && (value.code === null || typeof value.code === "number")
    && "started_at" in value && typeof value.started_at === "number")) return null;
  return { id: value.id, project: value.project, machine_id: value.machine_id, machine_name: value.machine_name,
    name: value.name, command: value.command, cwd: value.cwd, state: value.state, code: value.code, started_at: value.started_at };
}

/// A shell can finish starting after navigation released its tab.
export function rememberTerminal(key: string, tab: MediaTab, row: TerminalRow) {
  const pending = startingTerminals.get(key)?.filter(t => t !== tab) ?? [];
  if (pending.length) startingTerminals.set(key, pending); else startingTerminals.delete(key);
  tab.existing = row;
  if (openTabs().includes(tab)) { rememberPane(); return; }
  if (tab.closed) return;
  const saved = savedPane(key);
  if (!saved || typeof saved !== "object" || !("tabs" in saved) || !Array.isArray(saved.tabs)) return;
  saved.tabs = saved.tabs.map(item => item && typeof item === "object" && item.id === tab.id ? savedTab(tab) : item);
  writeView(windowPreferenceKey(key), saved);
}

window.addEventListener("pagehide", rememberPane);

export function loadLayout(): PaneLayout {
  try { return readLayout(readWindowPreference(KEY)); } catch { return readLayout(null); }
}

/// The layout to keep: a narrow window's fold of the right pane is not kept.
function keptLayout(): PaneLayout {
  const { layout, wideFolded } = paneModel.getState();
  return wideFolded === null ? layout : withFolded(layout, "right", wideFolded);
}

export function saveLayout() {
  try { writeWindowPreference(KEY, JSON.stringify(keptLayout())); } catch { /* kept while the window is open */ }
  rememberPane();
}
