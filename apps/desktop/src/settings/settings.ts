import { createElement } from "react";
import { flushSync } from "react-dom";
import { createRoot, type Root } from "react-dom/client";
import { AgentsOverview, AppsOverview, CodingAgentOverview, InstructionsOverview, MachinesOverview } from "./overview-view.tsx";
import { ModelSettingsView } from "./model-settings-view.tsx";
import { ServerView } from "./server-view.tsx";
import { GeneralView } from "./general-view.tsx";
import { AppearanceView, ComputerView } from "./local-view.tsx";
import { SettingsBreadcrumb, SettingsNavigation } from "./settings-shell-view.tsx";
import { onPhone } from "../ui/platform.ts";
import type { PanePage } from "../bar/pane.ts";
// settings/: the Settings pages (apps, accounts, credentials, coding agents,
// instructions, model) and the window's sheet (sheet.ts). Entry: this file.
import { bindAgents, disposeAgents, preserveAgents, bindCreds, disposeCreds, preserveCreds, renderCreds, CRED, loadAgentAccounts, loadCreds, setAgentsHtml, setCredsHtml } from "./accounts.ts";
import { A, type App, appsIn, appsOut, type AppsView, loadApps, type McpServer, renderAppPermissions, renderApps, renderMcpServer } from "./apps.ts";
import { toWindow } from "../chat/chat.ts";
import { bar, type LinkState, S, windowHooks } from "../bar/core.ts";
import { hints, refreshHints } from "../bar/hints.ts";
import { renderInvitePhone } from "../bar/link.ts";
import { type Machine, machinesIn, machinesOut, MC, renderMachines } from "../machines/machines.ts";
import { harnessName } from "./providers-state.ts";
import { back, CODING_AGENTS, fitPath, isPage, openSub, pageLabel, recallPath, rememberPath, samePath, selectPage, type SettingsPage, type SettingsPath, type SettingsSub } from "./settings-nav.ts";
import { canPickModel, type Models, type Thinking, MODELS } from "./model.ts";
import { show } from "../bar/views.ts";
import { type Reach } from "./server-state.ts";
import { esc } from "../ui/format.ts";
import { invoke, listen } from "../ui/tauri.ts";
import { toggleShortcuts } from "../ui/shortcutsheet.tsx";

/// What a new notice does while you are at this Mac (NoticeVoice).
export type NoticeVoice = "ask" | "aloud" | "off";

/// This Mac's own settings (`settings`).
export interface MacSettings {
  /// The name this Mac links with.
  name: string;
  version: string;
  autostart: boolean;
  notice_voice: NoticeVoice;
  /// In live mode a pause sends, rather than only "send".
  send_on_pause: boolean;
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

/// A device on the account (DeviceView).
export interface Device {
  id: string;
  name: string;
  kind: "phone" | "desktop";
  /// The device that made the account, which links and removes others.
  primary: boolean;
  this_device: boolean;
}

/// The devices Settings lists, and whether the server they came from answered (`list_devices`).
interface DeviceList {
  devices: Device[];
  server: Reach;
}

/// Settings: drawn at once from what this Mac knows (its own settings and the
/// last devices, apps and machines seen), then each server-backed section fills
/// in on its own as its call answers.
export const SET: {
  root: HTMLElement | null;
  token: number;
  devices: Device[] | null;
  machines: Machine[] | null | undefined;
  /// The question the section asks before it acts.
  confirm: false | "unlink" | "signout";
  /// Whether the server answered the last list of devices; null before it did.
  reach: Reach | null;
  modelsError?: boolean;
  path: SettingsPath | null;
  local: { set: MacSettings; mac: MacControl | null; linked: boolean } | null;
  unembed: (() => void) | null;
} = { root: null, token: 0, devices: null, machines: undefined, confirm: false, reach: null, path: null, local: null, unembed: null };

let modelSettingsBusy = false, modelSettingsError = "", modelSettingsVersion = 0;
async function setModelChange(command: string, args: Record<string, unknown>) {
  if (modelSettingsBusy) return;
  const owner = localViews.get("models");
  modelSettingsBusy = true; modelSettingsError = ""; modelSettingsVersion++;
  if (setShown()) mountLocalViews(setRoot());
  try {
    MODELS.info = await invoke<Models>(command, args);
    SET.modelsError = false;
    refreshHints();
  } catch (error) { if (owner && localViews.get("models") === owner) modelSettingsError = String(error); }
  finally { modelSettingsVersion++; modelSettingsBusy = false; if (setShown()) mountLocalViews(setRoot()); }
}

export const setRoot = () => SET.root ?? bar;
/// The sheet is open, or the bar still shows Settings.
export const setShown = () => SET.root ? SET.root.isConnected : S.view === "settings";

// Permissions are granted in System Settings; coming back shows them.
// Not while a token is typed in Credentials: the redraw would drop it.
// In the window the fields are in the sheet, not the pane.
window.addEventListener("focus", () => {
  const $set = (sel: string) => setRoot().querySelector<HTMLInputElement>(sel);
  if (setShown() && !$set("#unlinkYes") && !$set("#signOutYes") && !$set("#credSecret")?.value && !$set("#agSecret")?.value) renderSettings();
});

/// Rows that stand in for a section until the server answers.
export const setSkeleton = (n: number) => `<div class="dskel-rows">${[78, 56, 68, 48].slice(0, n).map(w => `<div class="dskel" style="width:${w}%"></div>`).join("")}</div>`;

/// Leaves Settings for another page: in the window the sheet closes and
/// the pane shows it.
function setAway(view: PanePage, inBar: () => unknown) {
  if (SET.root && windowHooks) return windowHooks.settingsAway(view);
  inBar();
}

type LocalView = "general" | "appearance" | "mac" | "devices" | "models" | "apps" | "machines" | "instr" | "agent-overview" | "agent-bar";
const generalPending = new Set<string>();
let macControlBusy = false;
const localViews = new Map<LocalView, { host: HTMLElement; root: Root; observer: MutationObserver }>();
function disposeLocalView(kind: LocalView) {
  const current = localViews.get(kind);
  localViews.delete(kind);
  if (kind === "models") modelSettingsError = "";
  if (!current) return;
  current.observer.disconnect();
  current.root.unmount();
}
function preserveLocalViews(container: HTMLElement) {
  const saved = [...localViews.entries()].filter(([, view]) => container.contains(view.host)).map(([kind, view]) => {
    const active = view.host.contains(document.activeElement) ? document.activeElement : null;
    view.host.remove();
    return { kind, view, active };
  });
  return (next?: HTMLElement) => {
    for (const { kind, view, active } of saved) {
      const replacement = next?.querySelector(`#set-${kind}`);
      if (!replacement) { disposeLocalView(kind); continue; }
      replacement.replaceWith(view.host);
      if (active instanceof HTMLElement) active.focus({ preventScroll: true });
    }
  };
}
function mountLocalViews(el: Element) {
  for (const kind of ["general", "appearance", "mac", "devices", "models", "apps", "machines", "instr", "agent-overview", "agent-bar"] as const) {
    const host = el.querySelector<HTMLElement>(`#set-${kind}`);
    if (!host) continue;
    let view = localViews.get(kind);
    if (view?.host !== host) {
      disposeLocalView(kind);
      const observer = new MutationObserver(() => { if (localViews.get(kind)?.host === host && !host.isConnected) disposeLocalView(kind); });
      observer.observe(document.body, { childList: true, subtree: true });
      view = { host, root: createRoot(host), observer };
      localViews.set(kind, view);
    }
    const local = SET.local, current = view;
    flushSync(() => current.root.render(kind === "general" && local ? createElement(GeneralView, {
      settings: local.set, phone: onPhone(), server: S.server, busy: generalPending,
      autostart: (enabled: boolean) => void changeGeneral("autostart", "set_autostart", { enabled }),
      voice: (choice: NoticeVoice) => void changeGeneral("voice", "set_notice_voice", { choice }),
      pause: (enabled: boolean) => void changeGeneral("pause", "set_send_on_pause", { enabled }),
      notices: (enabled: boolean) => void changeGeneral("notices", "set_agent_notices", { enabled }),
      shortcuts: () => windowHooks ? windowHooks.openShortcuts() : toggleShortcuts(),
      report: () => windowHooks ? windowHooks.openReport() : toWindow("report"),
    }) : kind === "apps" ? createElement(AppsOverview, { apps: A.apps, open: () => setAway("apps", renderApps) })
      : kind === "machines" ? createElement(MachinesOverview, { machines: SET.machines, open: () => setAway("machines", renderMachines) })
      : kind === "instr" ? createElement(InstructionsOverview, { open: () => windowHooks ? windowHooks.openInstructions() : toWindow("instructions") })
      : kind === "agent-bar" ? createElement(AgentsOverview, { open: () => windowHooks ? windowHooks.openProviders() : toWindow("providers") })
      : kind === "agent-overview" ? createElement(CodingAgentOverview, { open: (sub: SettingsSub) => goTo(openSub(sub)) })
      : kind === "models" ? createElement(ModelSettingsView, {
      model: MODELS.info, failed: !!SET.modelsError, busy: modelSettingsBusy, error: modelSettingsError,
      pick: (id: string) => void setModelChange("set_model", { id }),
      auto: (enabled: boolean) => void setModelChange("set_auto_model", { enabled }),
      thinking: (level: Thinking) => void setModelChange("set_thinking", { level }),
    }) : kind === "appearance" ? createElement(AppearanceView) : kind === "devices" && local ? createElement(ServerView, {
      settings: local.set, devices: SET.devices, link: S.link, signedOut: S.signedOut, reach: SET.reach,
      refresh: () => void renderSettings(), invite: () => setAway("invite", renderInvitePhone),
      away: () => { if (SET.root) windowHooks?.settingsAway(null); show(null); },
    }) : kind === "mac" && local?.mac ? createElement(ComputerView, {
      mac: local.mac, busy: macControlBusy,
      setEnabled: (enabled: boolean) => void setMacEnabled(enabled),
      allow: (permission: "ScreenRecording" | "Accessibility") => { invoke("allow_mac_permission", { permission }).catch(console.warn); },
    }) : null));
  }
}

async function changeGeneral(key: string, command: string, args: Record<string, unknown>) {
  if (generalPending.size) return;
  generalPending.add(key);
  if (setShown()) mountLocalViews(setRoot());
  try {
    try { await invoke(command, args); } catch (error) { console.warn(error); }
    if (setShown()) await renderSettings();
  } finally {
    generalPending.delete(key);
    if (setShown()) mountLocalViews(setRoot());
  }
}

async function setMacEnabled(enabled: boolean) {
  if (macControlBusy) return;
  macControlBusy = true;
  if (setShown()) mountLocalViews(setRoot());
  try {
    try { await invoke("set_mac_control", { enabled }); } catch (error) { console.warn(error); }
    if (setShown()) await renderSettings();
  } finally {
    macControlBusy = false;
    if (setShown()) mountLocalViews(setRoot());
  }
}

/// Binds the controls inside `el`, a section or the whole page.
function bindSettings(el: Element) {
  mountLocalViews(el);
  bindCreds(el);
  bindAgents(el);

}

/// Redraws one section in place, once its data came.
export function fillSection(id: string, html: string) {
  const box = setRoot().querySelector<HTMLElement>(`#set-${id}`);
  if (!box) return;
  const focus = focusKey(box);
  box.innerHTML = html;
  bindSettings(box);
  if (focus) refocus(box, focus);
}

// The data attributes that name a Settings control when it has no id.
const FOCUS_KEYS = ["remove", "vn", "pause", "allow", "goto", "sub", "setlevel"];

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

/// The sections of Settings, in order, each with the page it is.
function settingsSections(_set: MacSettings, mac: MacControl | null, linked: boolean, sheet: boolean): [SettingsPage, string][] {
  const sec = (id: string, title: string, body: string) => `<section id="set-${id}-sec" data-sec="${id}"><div class="dsec flat top">${title}</div><div id="set-${id}">${body}</div></section>`;
  // A page drawn by its own code, into this box.
  const host = (id: SettingsPage) => `<section id="set-${id}-sec" data-sec="${id}"><div class="dset-host" id="set-${id}-host"></div></section>`;
  const out: [SettingsPage, string][] = [["general", `<section id="set-general-sec" data-sec="general"><div class="dsec flat top">General</div><div id="set-general" style="display:contents"></div></section>`]];
  out.push(["appearance", sec("appearance", "Appearance", "")]);
  const machines: [SettingsPage, string] = ["machines", sheet ? host("machines") : sec("machines", "Machines", "")];
  const creds: [SettingsPage, string] = ["creds", sec("creds", "Credentials", setCredsHtml())];
  const instr: [SettingsPage, string] = ["instr", sheet ? host("instr") : sec("instr", "Instructions", "")];
  const sub = SET.path?.sub;
  const agents: [SettingsPage, string] = ["agents", sheet && sub?.kind === "agent" ? host("agents")
    : `<section id="set-agents-sec" data-sec="agents"><div class="dsec flat top">${linked ? "Agent accounts" : "Coding agents"}</div>${sheet ? "" : `<div id="set-agent-bar" style="display:contents"></div>`}${linked ? `<div id="set-agents">${setAgentsHtml()}</div>` : ""}${sheet ? `<div id="set-agent-overview" style="display:contents"></div>` : ""}</section>`];
  // These settings work over the mesh without the server.
  const thisMacSec: [SettingsPage, string][] = mac && !onPhone() ? [["mac", `<section id="set-mac-sec" data-sec="mac"><div class="dsec flat top">This computer</div><div id="set-mac" style="display:contents"></div></section>`]] : [];
  if (!linked) return [...out, ...thisMacSec, machines, agents, creds, instr, ...(S.server ? [["devices", sec("devices", "Beep server", "")] as [SettingsPage, string]] : [])];
  if (!MODELS.info || canPickModel()) out.push(["models", sec("models", "Models", "")]);
  out.push(["apps", sheet ? host("apps") : sec("apps", "Apps", "")]);
  out.push(...thisMacSec);
  out.push(machines);
  out.push(agents);
  out.push(creds, instr);
  out.push(["devices", sec("devices", "Beep server", "")]);
  return out;
}

/// What a subpage is called in the breadcrumb and the left pane.
function subLabel(sub: SettingsSub) {
  if (sub.kind === "app") return A.apps?.find(a => a.slug === sub.id)?.name ?? sub.id;
  if (sub.kind === "mcp") return A.servers?.find(s => s.id === sub.id)?.name ?? "MCP server";
  if (sub.kind === "machine") return (SET.machines ?? MC.list)?.find(m => m.id === sub.id)?.name ?? "Machine";
  return harnessName(sub.id);
}

/// Whether a remembered subpage still has something to show. Lists not
/// read yet say yes; the page itself checks once they come.
function subExists(sub: SettingsSub) {
  if (sub.kind === "app") return !A.apps || A.apps.some(a => a.slug === sub.id && a.connected);
  if (sub.kind === "mcp") return !A.servers || A.servers.some(s => s.id === sub.id && s.connected);

  if (sub.kind === "machine") {
    const list = SET.machines ?? MC.list;
    return !list || list.some(m => m.id === sub.id);
  }

  return true;
}

/// The subpages a page lists under it in the left pane, once picked.
function subEntries(page: SettingsPage): SettingsSub[] {
  if (page === "apps") return (A.apps ?? []).filter(a => a.connected).map(a => ({ kind: "app", id: a.slug }));
  if (page === "machines") return (SET.machines ?? MC.list ?? []).map(m => ({ kind: "machine", id: m.id }));
  if (page === "agents") return CODING_AGENTS.map(id => ({ kind: "agent", id }));
  return [];
}

/// A count beside a page in the left pane, such as connected apps.
function navCount(page: SettingsPage) {
  return (page === "apps" ? A.apps?.filter(app => app.connected).length : page === "machines" ? (SET.machines ?? MC.list)?.length : 0) ?? 0;
}
function settingsPageLabel(page: SettingsPage) {
  return page === "agents" && !SET.local?.linked ? "Coding agents" : pageLabel(page);
}

/// localStorage, or null where it cannot be reached.
function store() {
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

/// The pages the sheet lists now.
const sheetPages = () => SET.local ? settingsSections(SET.local.set, SET.local.mac, SET.local.linked, true).map(([id]) => id) : [];

let renderedPath: SettingsPath | null = null;
let shell: { sheet: HTMLElement; navigation: HTMLElement; navRoot: Root; breadcrumb: HTMLElement; crumbRoot: Root; observer: MutationObserver } | null = null;
function disposeShell() {
  const current = shell;
  shell = null;
  renderedPath = null;
  if (!current) return;
  current.observer.disconnect();
  current.navRoot.unmount(); current.crumbRoot.unmount();
}
function preserveShell(sheet: HTMLElement) {
  if (!shell || shell.sheet !== sheet) { disposeShell(); return null; }
  const scroll = { top: shell.navigation.scrollTop, left: shell.navigation.scrollLeft };
  shell.navigation.remove(); shell.breadcrumb.remove();
  return scroll;
}
function renderShell(sheet: HTMLElement, pages: SettingsPage[], path: SettingsPath, version: string) {
  if (shell && shell.sheet !== sheet) disposeShell();
  const nav = sheet.querySelector<HTMLElement>(".dset-nav"), crumb = sheet.querySelector<HTMLElement>(".dset-crumbs");
  if (!nav || !crumb) return;
  if (shell) {
    if (nav !== shell.navigation) nav.replaceWith(shell.navigation);
    if (crumb !== shell.breadcrumb) crumb.replaceWith(shell.breadcrumb);
  }
  else {
    const observer = new MutationObserver(() => { if (shell?.sheet === sheet && !sheet.isConnected) disposeShell(); });
    observer.observe(document.body, { childList: true, subtree: true });
    shell = { sheet, navigation: nav, navRoot: createRoot(nav), breadcrumb: crumb, crumbRoot: createRoot(crumb), observer };
  }
  const current = shell, openPage = (page: SettingsPage) => goTo(selectPage(page));
  flushSync(() => {
    current.navRoot.render(createElement(SettingsNavigation, { pages, path, version, subs: subEntries, pageName: settingsPageLabel, subName: subLabel, count: navCount, openPage, openSub: (sub: SettingsSub) => goTo(openSub(sub)) }));
    current.crumbRoot.render(createElement(SettingsBreadcrumb, { path, page: settingsPageLabel(path.page), sub: path.sub ? subLabel(path.sub) : null, openPage }));
  });
}
/// Draws the sheet's current page while keeping its navigation mounted.
function drawSheet() {
  const el = SET.root, local = SET.local;
  if (!el || !local) return;
  const sections = settingsSections(local.set, local.mac, local.linked, true);
  const shown = sections.map(([id]) => id);
  const path = SET.path = fitPath(SET.path ?? recallPath(store()), shown, subExists);
  rememberPath(store(), path);
  const page = sections.find(([id]) => id === path.page)?.[1] ?? "";
  const retainPage = shell?.sheet === el && renderedPath && samePath(renderedPath, path)
    && (path.page === "apps" || path.page === "machines" || path.page === "instr" || (path.page === "agents" && path.sub?.kind === "agent"));
  const oldPage = retainPage ? el.querySelector<HTMLElement>(".dset-page") : null;
  const previousFocus = document.activeElement instanceof HTMLElement && el.contains(document.activeElement) ? document.activeElement : null;
  const active = oldPage?.contains(document.activeElement) ? document.activeElement : null;
  const input = active instanceof HTMLInputElement || active instanceof HTMLTextAreaElement ? active : null;
  const start = input?.selectionStart ?? null, end = input?.selectionEnd ?? null;
  const scroll = oldPage ? { top: oldPage.scrollTop, left: oldPage.scrollLeft } : null;
  if (oldPage) oldPage.remove(); else leavePage();
  const focus = active ? null : focusKey(el);
  const restoreAgents = preserveAgents(), restoreCreds = preserveCreds(), restoreLocal = preserveLocalViews(el);
  const navScroll = preserveShell(el);
  el.innerHTML = `<nav class="dset-nav" aria-label="Settings pages"></nav>
    <div class="dset-body"><div class="dset-top"><span class="dset-crumbs"></span><kbd>esc</kbd></div>
    <div class="dset-page dform">${page}</div></div>`;
  renderShell(el, shown, path, local.set.version);
  restoreAgents(el); restoreCreds(el); restoreLocal(el);
  const body = el.querySelector<HTMLElement>(".dset-page");
  if (body && oldPage) body.replaceWith(oldPage);
  else { if (body) bindSettings(body); mountPage(path); }
  renderedPath = path;
  if (active instanceof HTMLElement && active.isConnected) {
    active.focus({ preventScroll: true });
    if (input && start !== null && end !== null) input.setSelectionRange(start, end);
  } else if (previousFocus?.isConnected) previousFocus.focus({ preventScroll: true });
  else if (focus) refocus(el, focus);
  if (oldPage && scroll) { oldPage.scrollTop = scroll.top; oldPage.scrollLeft = scroll.left; }
  if (navScroll && shell) { shell.navigation.scrollTop = navScroll.top; shell.navigation.scrollLeft = navScroll.left; }
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
  rememberPath(store(), path);
  drawChrome();
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
  appsOut();
  machinesOut();
  const unembed = SET.unembed;
  SET.unembed = null;
  unembed?.();
}

const followApps = (view: AppsView, id: string | null) =>
  follow("apps", view === "apps" || !id ? null : { kind: view === "mcp" ? "mcp" : "app", id });

/// Draws the page of `path` that its own code draws, into its box.
function mountPage(path: SettingsPath) {
  const box = SET.root?.querySelector<HTMLElement>(`#set-${path.page}-host`);
  if (!box) return;
  const sub = path.sub;

  if (path.page === "apps") {
    appsIn(box, followApps);
    return openApps(box, sub);
  }

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

/// Apps, or one connected app or server of it. A remembered one waits for
/// the list; one gone since shows the list.
async function openApps(box: HTMLElement, sub: SettingsSub | null) {
  if (!sub || (sub.kind !== "app" && sub.kind !== "mcp")) return renderApps();
  const find = (): App | McpServer | undefined => sub.kind === "app"
    ? A.apps?.find(a => a.slug === sub.id && a.connected)
    : A.servers?.find(s => s.id === sub.id && s.connected);

  if (!find()) {
    box.innerHTML = setSkeleton(3);
    await loadApps();
    if (!box.isConnected || !SET.path?.sub || !samePath(SET.path, openSub(sub))) return;
  }

  const it = find();
  if (!it) return goTo(selectPage("apps"));
  if (sub.kind === "app") return renderAppPermissions(it as App);
  return renderMcpServer(it as McpServer);
}

/// Draws Settings into `root` (the window's sheet), or the bar when none.
/// The sheet opens on `page` when given, else on the page picked last.
export async function renderSettings(confirm: false | "unlink" | "signout" = false, root?: HTMLElement, page?: string) {
  // A new sheet starts on `page`, or where the last one was left.
  if (root !== undefined && root !== SET.root) SET.path = isPage(page) ? selectPage(page) : recallPath(store());
  if (root !== undefined) SET.root = root;
  else if (SET.root && !SET.root.isConnected) SET.root = null;
  // The window shows Settings as its sheet, never as a page of the pane.
  if (!SET.root && windowHooks) return windowHooks.openSettings();
  if (!SET.root) S.view = "settings";
  SET.confirm = confirm;
  const token = ++SET.token;
  const alive = () => SET.token === token && setShown();
  // Local: answered by this Mac, never the server.
  const [got, mac] = await Promise.all([
    invoke<MacSettings>("settings"),
    invoke<MacControl>("mac_control").catch(e => { console.warn(e); return null; }),
  ]).catch((e: unknown) => [e, null] as const);
  if (!alive()) return;
  const el = setRoot();

  if (got instanceof Error || typeof got !== "object") {
    disposeAgents(); disposeCreds(); disposeShell(); for (const kind of localViews.keys()) disposeLocalView(kind);
    el.innerHTML = `<div class="dwin"><h4>Settings</h4><p class="derr">${esc(got)}</p></div>`;
    return;
  }

  // What `settings` answered: an object that is not an Error.
  const set = got as MacSettings;
  const linked = S.link === "linked";
  SET.local = { set, mac, linked };

  if (SET.root) {
    drawSheet();
  } else {
    disposeShell();
    const sections = settingsSections(set, mac, linked, false).map(([, html]) => html).join("");
    const restoreAgents = preserveAgents(), restoreCreds = preserveCreds(), restoreLocal = preserveLocalViews(el);
    el.innerHTML = `<div class="dhead"><span class="tt"><b>Settings</b><small>Beep desktop ${esc(set.version)}</small></span></div>
      <div class="dform dformcol">${sections}</div>${hints([["esc", "close"]])}`;
    restoreAgents(el); restoreCreds(el); restoreLocal(el);
    bindSettings(el);
  }

  // The server's parts, side by side; each fills its section as it answers.
  if (linked) loadServerParts(alive);
  invoke<Machine[]>("list_machines").then(m => { SET.machines = m; MC.list = m; }, () => { SET.machines = null; })
    .then(() => {
      if (!alive()) return;
      if (SET.root) drawChrome(); else mountLocalViews(setRoot());
      // The git hosts say which machines have gh and teacli.
      if (CRED.list) renderCreds();
    });
  loadCreds();
  if (linked) loadAgentAccounts();
}

/// The subscription's parts of Settings, which fill their sections as each answers.
function loadServerParts(alive: () => boolean) {
  invoke<DeviceList>("list_devices").then(d => { SET.devices = d.devices; SET.reach = d.server; }, e => { console.warn(e); SET.devices ??= []; SET.reach = "unreachable"; })
    .then(() => { if (alive()) mountLocalViews(setRoot()); });
  const modelVersion = modelSettingsVersion;
  invoke<Models>("get_models").then(m => { if (modelVersion === modelSettingsVersion) { MODELS.info = m; SET.modelsError = false; } }, e => { if (modelVersion === modelSettingsVersion) { console.warn(e); SET.modelsError = !MODELS.info; } })
    .then(() => {
      if (!alive() || modelVersion !== modelSettingsVersion) return;
      // A server with no choices to offer leaves the section out.
      if (canPickModel() || SET.modelsError) {
        if (SET.root) drawChrome();
        return mountLocalViews(setRoot());
      }
      if (SET.root) {
        if (SET.path?.page === "models") drawSheet();
        else drawChrome();
        return;
      }
      setRoot().querySelector("#set-models-sec")?.remove();
      setRoot().querySelector('[data-goto="models"]')?.remove();
    });
  invoke<App[]>("list_apps").then(a => { A.apps = a; }, () => { A.apps ??= []; })
    .then(() => {
      if (!alive()) return;
      if (SET.root) drawChrome(); else mountLocalViews(setRoot());
    });
}

// Signing in or out changes which pages Settings has. The state is taken here too, so the
// redraw never reads what the bar's own listener has not stored yet.
listen<boolean>("signed-out", e => { S.signedOut = e.payload; });
listen<LinkState>("link-state", e => { S.link = e.payload; if (setShown()) renderSettings(); });
