// Settings as a sheet over the command center window.
import { ago } from "../ui/format.ts";
import { dbtn as button } from "../ui/parts.ts";
import { invoke } from "../ui/tauri.ts";
import { pane } from "../bar/pane.ts";
import { el as h } from "../ui/dom.ts";
import { registerInstructions } from "./instructions.ts";
import { registerProviders } from "./providers.ts";
import { type ViewArg, views } from "../center/state.ts";
import { skeleton } from "../center/pieces.ts";
import { shown, shownArg, registerView, windowProject, toMain, drawTop } from "../center/views.ts";

/// Settings as a sheet over the window: a dimmed backdrop and a centered sheet
/// with its sections on the left.
let settingsOpen: { scrim: HTMLElement; keys: (e: KeyboardEvent) => void; back: HTMLElement | null } | null = null;

export function openSettings(section?: string) {
  // A project window has no Settings of its own: the main window opens them.
  if (windowProject) return toMain("settings");
  if (settingsOpen) return section && showSection(settingsOpen.scrim, section);
  const sheet = h("div", "cc-set");
  sheet.setAttribute("role", "dialog");
  sheet.setAttribute("aria-modal", "true");
  sheet.setAttribute("aria-label", "Settings");
  sheet.tabIndex = -1;
  const nav = h("nav", "dset-nav", h("b", "", "Settings"));
  const body = h("div", "dset-body", h("div", "dset-top", h("span", "tt", h("b", "", "Settings")), h("kbd", "", "esc")), ...skeleton([80, 60, 72, 50]));
  sheet.append(nav, body);
  const scrim = h("div", "cc-set-scrim", sheet);
  const back = document.activeElement as HTMLElement | null;
  scrim.addEventListener("mousedown", e => { if (e.target === scrim) closeSettings(); });
  // While open, keys stay in the sheet: Esc closes it, and nothing reaches
  // the page behind.
  const keys = (e: KeyboardEvent) => {
    // Esc on a subpage, such as Apps then Gmail, goes back to its page first.
    if (e.key === "Escape") { e.preventDefault(); if (!pane.settingsBack()) closeSettings(); }
    else if (e.metaKey && e.key === ",") e.preventDefault();
    else if (e.key === "Tab") trapTab(e, sheet);
    e.stopImmediatePropagation();
  };
  window.addEventListener("keydown", keys, true);
  document.body.append(scrim);
  settingsOpen = { scrim, keys, back };
  sheet.focus();
  pane.settings(sheet, section).catch(e => console.warn("settings:", e));
}

/// Shows page `section` of the open sheet, such as "machines".
function showSection(scrim: HTMLElement, section: string) {
  scrim.querySelector<HTMLElement>(`[data-goto="${section}"]`)?.click();
}

/// A window view the Settings sheet draws in its right pane, such as
/// Coding agents under Agent accounts, and where its header buttons go.
let embedded: { name: string; tools?: (nodes: Node[]) => void } | null = null;

/// Mounts view `name` into `root` in the sheet. What it returns mounts the
/// view back into its own element when the window shows it.
export function embedView(name: string, root: HTMLElement, arg: unknown, tools?: (nodes: Node[]) => void) {
  const view = views.get(name);
  if (!view) return () => {};
  const mine = { name, tools };
  unembed();
  embedded = mine;
  view.mount(root, arg as ViewArg);
  drawEmbedTools();
  return () => {
    if (embedded !== mine) return;
    unembed();
  };
}

function unembed() {
  const was = embedded;
  embedded = null;
  if (was && shown === was.name) {
    const view = views.get(was.name)!;
    view.mount(view.el, shownArg);
    drawTop();
  }
}

function drawEmbedTools() {
  const view = embedded?.tools && views.get(embedded.name);
  if (!view) return;
  const head = typeof view.title === "function" ? view.title() : null;
  embedded!.tools!(head?.tools ?? []);
}

export function closeSettings() {
  if (!settingsOpen) return;
  const { scrim, keys, back } = settingsOpen;
  settingsOpen = null;
  unembed();
  window.removeEventListener("keydown", keys, true);
  scrim.remove();
  if (back?.isConnected) back.focus();
}

/// Keeps Tab inside the sheet.
function trapTab(e: KeyboardEvent, sheet: HTMLElement) {
  const all = [...sheet.querySelectorAll<HTMLElement>("button:not([disabled]), [href], [tabindex]:not([tabindex='-1'])")];
  if (!all.length) return;
  const first = all[0], last = all[all.length - 1];
  // Focus lost to the page (a redraw removed it): bring it back in.
  if (!sheet.contains(document.activeElement)) { e.preventDefault(); return (e.shiftKey ? last : first).focus(); }
  if (e.shiftKey && (document.activeElement === first || document.activeElement === sheet)) { e.preventDefault(); last.focus(); }
  else if (!e.shiftKey && document.activeElement === last) { e.preventDefault(); first.focus(); }
}


registerInstructions({ registerView, invoke, h, button, ago, openSettings });
registerProviders({ registerView, invoke, h, button, openSettings, redrawTop: () => {
  if (shown === "providers") drawTop();
  if (embedded?.name === "providers") drawEmbedTools();
} });
