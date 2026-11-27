import { refreshTopicSubscriptions, subscribeEach, topic } from "../../ui/mesh-events.ts";
import { commandKey } from "../../ui/platform.ts";
// Settings as a sheet over the command center window.
import { formatAgo } from "../../ui/format.ts";
import { mountSheetFrame, dialogAbove } from "../../ui/parts.tsx";
import { invoke, recordTiming } from "../../ui/tauri.ts";
import { PageTiming } from "../../stores/center/page-timing.ts";
import { perfLayer, perfOpened } from "../../ui/perf.ts";
import { pane } from "../bar/pane.ts";
import type { ReactNode } from "react";
import { flushSync } from "react-dom";
import type { CenterView, ViewArg, ViewName } from "../../stores/center/state.ts";
import { toMain } from "../../parts/center/nav.ts";
import { boxOf, currentLocation, windowAlone, drawTop } from "../../parts/center/nav.ts";

let settingsMod: typeof import("../../screens/settings/settings.tsx") | null = null;
async function ensureSettingsMod() {
  if (!settingsMod) {
    const loaded = await import("../../screens/settings/settings.tsx");
    loaded.connectSettingsSheet({
      show: (_sheet, content) => { const open = settingsOpen; if (open) flushSync(() => open.render(content)); },
      close: closeSettings,
    });
    settingsMod = loaded;
  }
  return settingsMod;
}

/// Settings as a sheet over the window: a dimmed backdrop and a centered sheet
/// with its sections on the left.
let settingsOpen: { scrim: HTMLElement; keys: (e: KeyboardEvent) => void; back: HTMLElement | null; render(content: ReactNode): void; dispose(): void } | null = null;
const settingsTiming = new PageTiming(() => performance.now(), callback => { requestAnimationFrame(callback); }, () => !document.hidden, (page, ms) => { if (settingsOpen) { recordTiming("page", page, ms); perfOpened("settings", ms); } });

export function openSettings(section?: string | null) {
  // A project window has no Settings of its own: the main window opens them.
  if (windowAlone) return toMain("settings");
  // Already open: show page `section`, such as "machines".
  if (settingsOpen) {
    if (section) void ensureSettingsMod().then(m => m.showSettingsPage(section));
    return;
  }
  const afterMount = settingsTiming.start("settings");
  perfLayer("settings", true);
  const { root: scrim, sheet, render, dispose } = mountSheetFrame("Settings", "cc-set", {
    host: document.body, scrimClass: "cc-set-scrim", onScrim: closeSettings, ownKeys: true,
  });
  const back = document.activeElement as HTMLElement | null;
  // While open, keys stay in the sheet: Esc closes it, and nothing reaches
  // the page behind.
  const keys = (e: KeyboardEvent) => {
    if (dialogAbove(scrim)) return;

    // Esc on a subpage, such as Apps then Gmail, goes back to its page first.
    if (e.key === "Escape") { e.preventDefault(); if (!pane.settingsBack()) closeSettings(); }
    else if (e.key === "/" && !e.metaKey && !e.ctrlKey && !e.altKey && !isTypingTarget(e.target)) {
      const find = sheet.querySelector<HTMLInputElement>(".dset-find");
      if (find) { e.preventDefault(); find.focus(); find.select(); }
    }
    else if (commandKey(e) && e.key === ",") e.preventDefault();
    else if (e.key === "Tab") trapTab(e, sheet);
    else if (e.target instanceof Node && sheet.contains(e.target)) return;

    e.stopImmediatePropagation();
  };
  window.addEventListener("keydown", keys, true);
  settingsOpen = { scrim, keys, back, render, dispose };
  sheet.focus();
  void ensureSettingsMod()
    .then(() => pane.settings(sheet, section))
    .then(() => { refreshTopicSubscriptions(); afterMount(); })
    .catch(e => console.warn("settings:", e));
}

/// Opens Settings on machine `id`'s page.
export function openMachineSettings(id: string) {
  void ensureSettingsMod().then(m => {
    m.showSettingsMachine(id);
    openSettings();
  });
}

/// A window view temporarily mounted in the Settings sheet, by the request that mounted it.
/// Unmounting the sheet restores it only when the underlying page still shows that view.
let embedded: { name: ViewName; token: object } | null = null;

/// Mounts view `name` into `root` in the sheet, for the request `token`.
export function embedView(name: ViewName, root: HTMLElement, arg: ViewArg, token: object) {
  const view = embeddable(name);
  if (!view) return;
  unembed();
  embedded = { name, token };
  view.mount(root, arg);
}

/// Mounts the view of request `token` back into its own element, unless another took the sheet.
export function unembedView(token: object) {
  if (embedded?.token === token) unembed();
}

/// The pages Settings shows inside its sheet.
function embeddable(name: ViewName): CenterView | null {
  switch (name) {
    case "providers": return providersView;
    case "instructions": return instructionsView;
    default: return null;
  }
}

function unembed() {
  const was = embedded;
  embedded = null;
  const page = currentLocation();

  if (!was) return;
  const view = embeddable(was.name), box = boxOf(was.name);
  if (!view) return;

  if (page && page.view === was.name && box) {
    view.mount(box, page.arg);
    drawTop();
  } else {
    view.unmount?.();
  }
}

export function closeSettings() {
  if (!settingsOpen) return;
  const { keys, back, dispose } = settingsOpen;
  settingsOpen = null;
  perfLayer("settings", false);
  settingsMod?.settingsClosed();
  unembed();
  window.removeEventListener("keydown", keys, true);
  dispose();
  refreshTopicSubscriptions();
  if (back?.isConnected) back.focus();
}

/// Keeps Tab inside the sheet.
function isTypingTarget(target: EventTarget | null) {
  return target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement || target instanceof HTMLSelectElement || (target instanceof HTMLElement && target.isContentEditable);
}

function trapTab(e: KeyboardEvent, sheet: HTMLElement) {
  const all = [...sheet.querySelectorAll<HTMLElement>("button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [href], [tabindex]:not([tabindex='-1'])")];
  if (!all.length) return;
  const first = all[0], last = all[all.length - 1];
  // Focus lost to the page (a redraw removed it): bring it back in.
  if (!sheet.contains(document.activeElement)) { e.preventDefault(); return (e.shiftKey ? last : first).focus(); }

  if (e.shiftKey && (document.activeElement === first || document.activeElement === sheet)) { e.preventDefault(); last.focus(); }
  else if (!e.shiftKey && document.activeElement === last) { e.preventDefault(); first.focus(); }
}

let loadedInstructions: CenterView<"instructions"> | null = null;
export const instructionsView: CenterView<"instructions"> = {
  title: () => loadedInstructions
    ? (typeof loadedInstructions.title === "function" ? loadedInstructions.title() : { title: loadedInstructions.title ?? "Instructions" })
    : { title: "Instructions", sub: "Your rules for every agent Beep runs" },
  async mount(el, arg) {
    loadedInstructions ??= (await import("../../screens/settings/instructions.tsx")).instructionsPage({ invoke, ago: ms => formatAgo(ms, "short"), openSettings });
    await loadedInstructions.mount(el, arg);
    if (currentLocation()?.view === "instructions") drawTop();
  },
  unmount: () => loadedInstructions?.unmount?.(),
};

let loadedProviders: CenterView<"providers"> | null = null;
export const providersView: CenterView<"providers"> = {
  title: () => loadedProviders
    ? (typeof loadedProviders.title === "function" ? loadedProviders.title() : { title: loadedProviders.title ?? "Coding agents" })
    : { title: "Coding agents" },
  async mount(el, arg) {
    loadedProviders ??= (await import("../../screens/settings/providers.tsx")).providersPage({ invoke, openSettings, redrawTop: () => {
      if (currentLocation()?.view === "providers") drawTop();
    }, follow: take => { subscribeEach([topic("machines"), topic("accounts")], event => take(event.data)); } });
    await loadedProviders.mount(el, arg);
    if (currentLocation()?.view === "providers") drawTop();
  },
  unmount: () => loadedProviders?.unmount?.(),
};
