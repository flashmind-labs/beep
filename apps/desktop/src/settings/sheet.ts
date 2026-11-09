import { refreshTopicSubscriptions } from "../ui/mesh-events.ts";
import { commandKey } from "../ui/platform.ts";
// Settings as a sheet over the command center window.
import { ago } from "../ui/format.ts";
import { dbtn as button, sheetAbove } from "../ui/parts.ts";
import { invoke, recordTiming } from "../ui/tauri.ts";
import { PageTiming } from "../center/page-timing.ts";
import { pane } from "../bar/pane.ts";
import { el as h } from "../ui/dom.ts";
import { registerInstructions } from "./instructions.ts";
import { registerProviders } from "./providers.ts";
import { type ViewArg, type ViewName, views } from "../center/state.ts";
import { skeleton } from "../ui/pieces.ts";
import { currentLocation, registerView, windowProject, toMain, drawTop } from "../center/views.ts";

/// Settings as a sheet over the window: a dimmed backdrop and a centered sheet
/// with its sections on the left.
let settingsOpen: { scrim: HTMLElement; keys: (e: KeyboardEvent) => void; back: HTMLElement | null } | null = null;
const settingsTiming = new PageTiming(() => performance.now(), callback => { requestAnimationFrame(callback); }, () => !document.hidden, (page, ms) => { if (settingsOpen) recordTiming("page", page, ms); });

export function openSettings(section?: string) {
  // A project window has no Settings of its own: the main window opens them.
  if (windowProject) return toMain("settings");
  // Already open: show page `section`, such as "machines".
  if (settingsOpen) return section && settingsOpen.scrim.querySelector<HTMLElement>(`[data-goto="${section}"]`)?.click();
  const afterMount = settingsTiming.start("settings");
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
    if (sheetAbove(scrim)) return;

    // Esc on a subpage, such as Apps then Gmail, goes back to its page first.
    if (e.key === "Escape") { e.preventDefault(); if (!pane.settingsBack()) closeSettings(); }
    else if (commandKey(e) && e.key === ",") e.preventDefault();
    else if (e.key === "Tab") trapTab(e, sheet);
    else if (e.target instanceof Node && sheet.contains(e.target)) return;

    e.stopImmediatePropagation();
  };
  sheet.addEventListener("keydown", e => e.stopPropagation());
  window.addEventListener("keydown", keys, true);
  document.body.append(scrim);
  settingsOpen = { scrim, keys, back };
  sheet.focus();
  pane.settings(sheet, section).then(() => { refreshTopicSubscriptions(); afterMount(); }).catch(e => console.warn("settings:", e));
}

/// A window view the Settings sheet draws in its right pane, such as
/// Coding agents under Agent accounts, and where its header buttons go.
let embedded: { name: ViewName; tools?: (nodes: Node[]) => void } | null = null;

/// Mounts view `name` into `root` in the sheet. What it returns mounts the
/// view back into its own element when the window shows it.
export function embedView(name: ViewName, root: HTMLElement, arg: ViewArg, tools?: (nodes: Node[]) => void) {
  const view = views.get(name);
  if (!view) return () => {};
  const mine = { name, tools };
  unembed();
  embedded = mine;
  view.mount(root, arg);
  drawEmbedTools();
  return () => {
    if (embedded !== mine) return;
    unembed();
  };
}

function unembed() {
  const was = embedded;
  embedded = null;
  const page = currentLocation();

  if (!was) return;
  const view = views.get(was.name);
  if (!view) return;

  if (page && page.view === was.name) {
    view.mount(view.el, page.arg);
    drawTop();
  } else {
    view.unmount?.();
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
  refreshTopicSubscriptions();
  if (back?.isConnected) back.focus();
}

/// Keeps Tab inside the sheet.
function trapTab(e: KeyboardEvent, sheet: HTMLElement) {
  const all = [...sheet.querySelectorAll<HTMLElement>("button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [href], [tabindex]:not([tabindex='-1'])")];
  if (!all.length) return;
  const first = all[0], last = all[all.length - 1];
  // Focus lost to the page (a redraw removed it): bring it back in.
  if (!sheet.contains(document.activeElement)) { e.preventDefault(); return (e.shiftKey ? last : first).focus(); }

  if (e.shiftKey && (document.activeElement === first || document.activeElement === sheet)) { e.preventDefault(); last.focus(); }
  else if (!e.shiftKey && document.activeElement === last) { e.preventDefault(); first.focus(); }
}

registerInstructions({ registerView, invoke, h, button, ago, openSettings });
registerProviders({ registerView, invoke, h, button, openSettings, redrawTop: () => {
  if (currentLocation()?.view === "providers") drawTop();
  if (embedded?.name === "providers") drawEmbedTools();
} });
