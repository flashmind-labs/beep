// settings/: the Settings pages (credentials, coding agents, instructions,
// machines) and the window's sheet (sheet.ts). Entry: this file.
import { createElement, createRef, type ReactNode, type ReactElement } from "react";
import { flushSync } from "react-dom";
import { AgentsOverview, CodingAgentOverview, InstructionsOverview, MachinesOverview } from "./overview-view.tsx";
import { GeneralView } from "./general-view.tsx";
import { AppearanceView, ComputerView } from "./local-view.tsx";
import { SettingsBreadcrumb, SettingsNavigation, SettingsShell, SettingsSection, SettingsHost, AgentSettingsSection, SettingsFailure, SettingsLoading } from "./settings-shell-view.tsx";
import { onPhone } from "../ui/platform.ts";
import type { PanePage } from "../bar/pane.ts";
import { agentAccountsContent, credentialsContent, renderCreds, CRED, loadAgentAccounts, loadCreds } from "./accounts.ts";
import { bar, S, toWindow, windowHooks } from "../bar/core.ts";
import { type BarPageSlot, openBarPage } from "../bar/bar-app.tsx";
import { hintModel } from "../bar/hints.ts";
import { type Machine, machinesIn, machinesOut, MC, renderMachines } from "../machines/machines.ts";
import { harnessName } from "./providers-state.ts";
import { back, CODING_AGENTS, fitPath, isPage, openSub, pageLabel, recallPath, rememberPath, samePath, selectPage, type SettingsPage, type SettingsPath, type SettingsSub } from "./settings-nav.ts";
import { invoke } from "../ui/tauri.ts";
import { toggleShortcuts } from "../ui/shortcutsheet.tsx";

/// This Mac's own settings (`settings`).
export interface MacSettings {
  /// The name this Mac links with.
  name: string;
  version: string;
  autostart: boolean;
  /// Coding agents notify when they ask something or finish a turn.
  agent_notices: boolean;
}

/// Settings, This Mac (`mac_control`): whether Beep may use it, where its
/// link stands and the permissions desktop control needs.
export interface MacControl {
  id: string;
  name: string;
  enabled: boolean;
  status: "off" | "connecting" | "connected" | "failed" | "unlinked";
  /// Why the last try failed, while `status` is "failed".
  error: string | null;
  permissions: { screen_recording: boolean; accessibility: boolean };
}

/// Settings: drawn at once from what this Mac knows (its own settings and the
/// last machines seen), then the machines and credentials fill in as they answer.
export const SET: {
  root: HTMLElement | null;
  token: number;
  machines: Machine[] | null | undefined;
  path: SettingsPath | null;
  local: { set: MacSettings; mac: MacControl | null } | null;
  unembed: (() => void) | null;
} = { root: null, token: 0, machines: undefined, path: null, local: null, unembed: null };

/// Draws Settings into `root` (the window's sheet), or the bar when none.
/// The sheet opens on `page` when given, else on the page picked last.
export async function renderSettings(root?: HTMLElement, page?: string) {
  // A new sheet starts on `page`, or where the last one was left.
  if (root !== undefined && root !== SET.root) SET.path = isPage(page) ? selectPage(page) : recallPath(store());
  if (root !== undefined) SET.root = root;
  else if (SET.root && !SET.root.isConnected) SET.root = null;
  // The window shows Settings as its sheet, never as a page of the pane.
  if (!SET.root && windowHooks) return windowHooks.openSettings();
  if (!SET.root) S.view = "settings";
  if (root && shownSheet !== root) showSheet(root, createElement(SettingsLoading));
  const token = ++SET.token;
  const alive = () => SET.token === token && setShown();
  const [got, mac] = await Promise.all([
    invoke<MacSettings>("settings"),
    invoke<MacControl>("mac_control").catch(e => { console.warn(e); return null; }),
  ]).catch((e: unknown) => [e, null] as const);
  if (!alive()) return;

  if (got instanceof Error || typeof got !== "object") {
    SET.local = null;
    renderedPath = null;
    leavePage();
    if (SET.root) showSheet(SET.root, createElement(SettingsFailure, { error: String(got) }));
    else flushSync(() => openBarPage(() => { barSettings = null; }).show({ view: "settings-failure", error: String(got) }));
    return;
  }

  // What `settings` answered: an object that is not an Error.
  const set = got as MacSettings;
  SET.local = { set, mac };

  if (SET.root) {
    drawSheet();
  } else {
    barSettings ??= openBarPage(() => { barSettings = null; });
    drawBarSettings();
  }

  invoke<Machine[]>("list_machines").then(m => { SET.machines = m; MC.list = m; }, () => { SET.machines = null; })
    .then(() => {
      if (!alive()) return;
      redrawSettings();
      // The git hosts say which machines have gh and teacli.
      if (CRED.list) renderCreds();
    });
  loadCreds();
  loadAgentAccounts();
}

/// Draws Settings again where it is shown, the sheet or the bar.
export function redrawSettings() {
  if (!setShown()) return;
  if (SET.root) drawChrome();
  else drawBarSettings();
}

export const setRoot = () => SET.root ?? bar;
/// The sheet is open, or the bar still shows Settings.
export const setShown = () => SET.root ? SET.root.isConnected : S.view === "settings";

/// The pages the sheet lists now.
const sheetPages = () => SET.local ? settingsSections(SET.local.mac, true).map(([id]) => id) : [];

/// The box of the page whose own code draws it (Machines, the embedded views).
const pageHost = createRef<HTMLDivElement>();

/// The sheet's frame, which Settings renders its content in. The window
/// hands it over (sheet.ts), so the bar never loads the window's modules.
export interface SettingsSheetOwner { show(sheet: HTMLElement, content: ReactNode): void }
let sheetOwner: SettingsSheetOwner | null = null;
export function connectSettingsSheet(owner: SettingsSheetOwner) { sheetOwner = owner; }

// `shownSheet` is the sheet Settings is drawn in.
let renderedPath: SettingsPath | null = null;
let shownSheet: HTMLElement | null = null;
function showSheet(sheet: HTMLElement, content: ReactNode) {
  shownSheet = sheet;
  sheetOwner?.show(sheet, content);
}
function renderShell(sheet: HTMLElement, pages: SettingsPage[], path: SettingsPath, version: string) {
  const openPage = (page: SettingsPage) => goTo(selectPage(page));
  showSheet(sheet, createElement(SettingsShell, {
    navigation: createElement(SettingsNavigation, { pages, path, version, subs: subEntries, pageName: pageLabel, subName: subLabel, count: navCount, openPage, openSub: (sub: SettingsSub) => goTo(openSub(sub)) }),
    breadcrumb: createElement(SettingsBreadcrumb, { path, page: pageLabel(path.page), sub: path.sub ? subLabel(path.sub) : null, openPage }),
    children: settingsSections(SET.local?.mac ?? null, true).find(([page]) => page === path.page)?.[1] ?? null,
  }));
}

/// The sheet closed: its pages stop drawing; its content goes with its frame.
export function settingsClosed() {
  if (!shownSheet) return;
  shownSheet = null;
  renderedPath = null;
  leavePage();
}
/// Draws the sheet's current page while keeping its navigation mounted.
function drawSheet() {
  const el = SET.root, local = SET.local;
  if (!el || !local) return;
  const sections = settingsSections(local.mac, true);
  const shown = sections.map(([id]) => id);
  const path = SET.path = fitPath(SET.path ?? recallPath(store()), shown, subExists);
  rememberPath(store(), path);
  const changed = shownSheet !== el || !renderedPath || !samePath(renderedPath, path);
  const previousFocus = document.activeElement instanceof HTMLElement && el.contains(document.activeElement) ? document.activeElement : null;
  const focus = focusKey(el);
  if (changed) leavePage();
  renderShell(el, shown, path, local.set.version);
  if (changed) mountPage(path);
  renderedPath = path;
  if (previousFocus?.isConnected) previousFocus.focus({ preventScroll: true });
  else if (focus) refocus(el, focus);
}
function drawChrome() {
  const el = SET.root, path = SET.path, local = SET.local;
  if (!el?.isConnected || !path || !local) return;
  renderShell(el, sheetPages(), path, local.set.version);
}

/// Shows `path` in the sheet.
function goTo(path: SettingsPath) {
  if (!SET.root?.isConnected) return;
  if (SET.path && samePath(SET.path, path)) return;
  SET.path = path;
  drawSheet();
}

/// A page drawn by its own code opened `sub` (null: its page) under `page`.
function follow(page: SettingsPage, sub: SettingsSub | null) {
  if (!SET.root?.isConnected) return;
  const path = sub ? openSub(sub) : selectPage(page);
  if (SET.path && samePath(SET.path, path)) return;
  SET.path = path;
  renderedPath = path;
  rememberPath(store(), path);
  drawChrome();
}

/// The open sheet shows `page`, such as "machines".
export function showSettingsPage(page: string) {
  if (isPage(page)) goTo(selectPage(page));
}

/// Esc in the sheet: from a subpage back to its page. False when on a page.
export function settingsBack() {
  if (!SET.root?.isConnected || !SET.path) return false;
  const to = back(SET.path);
  if (!to) return false;
  goTo(to);
  return true;
}

/// Stops the pages that draw themselves from drawing into the sheet.
function leavePage() {
  machinesOut();
  const unembed = SET.unembed;
  SET.unembed = null;
  unembed?.();
}

/// Draws the page of `path` that its own code draws, into its box.
function mountPage(path: SettingsPath) {
  const sub = path.sub;
  const box = pageHost.current;
  if (!box) return;

  if (path.page === "machines") {
    machinesIn(box, id => follow("machines", id ? { kind: "machine", id } : null), () => { SET.machines = MC.list; drawChrome(); });
    return renderMachines(sub?.kind === "machine" ? sub.id : null);
  }

  const embed = windowHooks?.embedView;
  if (!embed) return;

  if (path.page === "agents" && sub?.kind === "agent") {
    // The view draws its machine picker in a toolbar of its own, under the breadcrumb.
    SET.unembed = embed("providers", box, { agent: sub.id, settings: true });
  } else if (path.page === "instr") {
    SET.unembed = embed("instructions", box, null);
  }
}

let barSettings: BarPageSlot | null = null;
function drawBarSettings() {
  const current = barSettings, local = SET.local;
  if (!current || !local) return;
  const sections = settingsSections(local.mac, false).map(([, content]) => content);
  flushSync(() => current.show({ view: "settings", props: { version: local.set.version, sections, hints: hintModel([["esc", "close"]]) } }));
}

/// The sections of Settings, in order, each with the page it is.
function settingsSections(mac: MacControl | null, sheet: boolean): [SettingsPage, ReactElement][] {
  const sec = (page: LocalView & SettingsPage, title: string) => createElement(SettingsSection, { key: page, page, title, children: localView(page) });
  const host = (page: SettingsPage) => createElement(SettingsHost, { key: page, page, hostRef: pageHost });
  const sub = SET.path?.sub;
  const agents = sheet && sub?.kind === "agent" ? host("agents")
    : createElement(AgentSettingsSection, { key: "agents", bar: sheet ? null : localView("agent-bar"), accounts: agentAccountsContent(), overview: sheet ? localView("agent-overview") : null });
  return [
    ["general", sec("general", "General")],
    ["appearance", sec("appearance", "Appearance")],
    ...(mac && !onPhone() ? [["mac", sec("mac", "This computer")] as [SettingsPage, ReactElement]] : []),
    ["machines", sheet ? host("machines") : sec("machines", "Machines")],
    ["agents", agents],
    ["creds", sec("creds", "Credentials")],
    ["instr", sheet ? host("instr") : sec("instr", "Instructions")],
  ];
}

/// What a subpage is called in the breadcrumb and the left pane.
function subLabel(sub: SettingsSub) {
  if (sub.kind === "machine") return (SET.machines ?? MC.list)?.find(m => m.id === sub.id)?.name ?? "Machine";
  return harnessName(sub.id);
}

/// Whether a remembered subpage still has something to show. Lists not
/// read yet say yes; the page itself checks once they come.
function subExists(sub: SettingsSub) {
  if (sub.kind === "machine") {
    const list = SET.machines ?? MC.list;
    return !list || list.some(m => m.id === sub.id);
  }

  return true;
}

/// The subpages a page lists under it in the left pane, once picked.
function subEntries(page: SettingsPage): SettingsSub[] {
  if (page === "machines") return (SET.machines ?? MC.list ?? []).map(m => ({ kind: "machine", id: m.id }));
  if (page === "agents") return CODING_AGENTS.map(id => ({ kind: "agent", id }));
  return [];
}

/// A count beside a page in the left pane: the machines.
function navCount(page: SettingsPage) {
  return (page === "machines" ? (SET.machines ?? MC.list)?.length : 0) ?? 0;
}

/// localStorage, or null where it cannot be reached.
function store() {
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

/// Leaves Settings for another page: in the window the sheet closes and
/// the pane shows it.
function setAway(view: PanePage, inBar: () => unknown) {
  if (SET.root && windowHooks) return windowHooks.settingsAway(view);
  inBar();
}

const generalPending = new Set<string>();
let macControlBusy = false;

/// What a Settings section shows, drawn fresh from the module state on each render.
type LocalView = "general" | "appearance" | "mac" | "machines" | "instr" | "creds" | "agent-overview" | "agent-bar";
function localView(kind: LocalView): ReactNode {
  const local = SET.local;
  switch (kind) {
    case "general": return local ? createElement(GeneralView, {
      settings: local.set, phone: onPhone(), busy: generalPending,
      autostart: (enabled: boolean) => void changeGeneral("autostart", "set_autostart", { enabled }),
      notices: (enabled: boolean) => void changeGeneral("notices", "set_agent_notices", { enabled }),
      shortcuts: () => windowHooks ? windowHooks.openShortcuts() : toggleShortcuts(),
      report: () => windowHooks ? windowHooks.openReport() : toWindow("report"),
    }) : null;
    case "appearance": return createElement(AppearanceView);
    case "mac": return local?.mac ? createElement(ComputerView, {
      mac: local.mac, busy: macControlBusy,
      setEnabled: (enabled: boolean) => void setMacEnabled(enabled),
      allow: (permission: "ScreenRecording" | "Accessibility") => { invoke("allow_mac_permission", { permission }).catch(console.warn); },
    }) : null;
    case "machines": return createElement(MachinesOverview, { machines: SET.machines, open: () => setAway("machines", renderMachines) });
    case "instr": return createElement(InstructionsOverview, { open: () => windowHooks ? windowHooks.openInstructions() : toWindow("instructions") });
    case "creds": return credentialsContent();
    case "agent-bar": return createElement(AgentsOverview, { open: () => windowHooks ? windowHooks.openProviders() : toWindow("providers") });
    case "agent-overview": return createElement(CodingAgentOverview, { open: (sub: SettingsSub) => goTo(openSub(sub)) });
  }
}

async function changeGeneral(key: string, command: string, args: Record<string, unknown>) {
  if (generalPending.size) return;
  generalPending.add(key);
  redrawSettings();
  try {
    try { await invoke(command, args); } catch (error) { console.warn(error); }
    if (setShown()) await renderSettings();
  } finally {
    generalPending.delete(key);
    redrawSettings();
  }
}

async function setMacEnabled(enabled: boolean) {
  if (macControlBusy) return;
  macControlBusy = true;
  redrawSettings();
  try {
    try { await invoke("set_mac_control", { enabled }); } catch (error) { console.warn(error); }
    if (setShown()) await renderSettings();
  } finally {
    macControlBusy = false;
    redrawSettings();
  }
}

// The data attributes that name a Settings control when it has no id.
const FOCUS_KEYS = ["remove", "allow", "goto", "sub"];

/// What has focus inside `box`, as a selector that finds it again after
/// a redraw; null when focus is elsewhere.
function focusKey(box: HTMLElement) {
  const a = document.activeElement as HTMLElement | null;
  // A button disabled while it waited drops focus to the body; in the
  // sheet that would let Tab reach the page behind.
  if (!a || a === document.body) return box.closest(".cc-set") ? "*" : null;
  if (a === box || !box.contains(a)) return null;
  if (a.id) return `#${CSS.escape(a.id)}`;
  if (a.matches('#skillSyncSetting [role="switch"]')) return '#skillSyncSetting [role="switch"]';
  if (a.dataset.skinPick) return `[data-skin-pick="${CSS.escape(a.dataset.skinPick)}"]`;
  const k = FOCUS_KEYS.find(k => k in a.dataset);
  return k ? `[data-${k}="${CSS.escape(a.dataset[k]!)}"]` : "*";
}

/// Puts focus back after a redraw, on the same control when it is still
/// there, else on the sheet, so Tab never lands on the page behind.
function refocus(box: HTMLElement, key: string) {
  const again = key === "*" ? null : box.querySelector<HTMLButtonElement>(key);
  if (again && !again.disabled) return again.focus();
  (box.closest<HTMLElement>(".cc-set") || box).focus();
}

// Permissions are granted in System Settings; coming back shows them.
// Not while a token is typed in Credentials: the redraw would drop it.
// In the window the fields are in the sheet, not the pane.
window.addEventListener("focus", () => {
  const $set = (sel: string) => setRoot().querySelector<HTMLInputElement>(sel);
  if (setShown() && !$set("#credSecret")?.value && !$set("#agSecret")?.value) renderSettings();
});
