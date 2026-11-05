// settings/: the Settings pages (apps, accounts, credentials, coding agents,
// instructions, model) and the window's sheet (sheet.ts). Entry: this file.
import { bindAgents, bindCreds, CRED, loadAgentAccounts, loadCreds, setAgentsHtml, setCredsHtml } from "./accounts.ts";
import { A, type App, appsIn, appsOut, type AppsView, loadApps, type McpServer, renderAppPermissions, renderApps, renderMcpServer } from "./apps.ts";
import { toWindow } from "../chat/chat.ts";
import { bar, S, windowHooks } from "../bar/core.ts";
import { hints, refreshHints } from "../bar/hints.ts";
import { renderInvitePhone } from "../bar/link.ts";
import { type Machine, machinesIn, machinesOut, MC, renderMachines } from "../machines/machines.ts";
import { harnessTile } from "../ui/harnessicon.ts";
import { harnessName } from "./providers-state.ts";
import { SKINS, setSkin, skinId } from "../ui/skin.ts";
import { back, CODING_AGENTS, crumbs, fitPath, isPage, navEntries, openSub, pageLabel, parseSub, recallPath, rememberPath, samePath, selectPage, type SettingsPage, type SettingsPath, type SettingsSub, subKey } from "./settings-nav.ts";
import { canPickModel, type ModelChoice, type Models, modelPrice, MODELS, THINKING } from "./model.ts";
import { cap } from "../chat/tools.ts";
import { show } from "../bar/views.ts";
import { SUBSCRIPTION_NOTE, signInWay } from "./subscription.ts";
import { switchHtml, esc } from "../ui/format.ts";
import { ICON } from "../ui/icons.ts";
import { invoke, listen } from "../ui/tauri.ts";

/// What a new notice does while you are at this Mac (NoticeVoice).
type NoticeVoice = "ask" | "aloud" | "off";

/// This Mac's own settings (`settings`).
interface MacSettings {
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
interface MacControl {
  id: string;
  name: string;
  enabled: boolean;
  status: "off" | "connecting" | "connected" | "failed" | "unlinked";
  /// Why the last try failed, while `status` is "failed".
  error: string | null;
  permissions: { screen_recording: boolean; accessibility: boolean };
}

/// A device on the account (DeviceView).
interface Device {
  id: string;
  name: string;
  kind: "phone" | "desktop";
  /// The device that made the account, which links and removes others.
  primary: boolean;
  this_device: boolean;
}

/// Settings, This Mac: the switch that lets Beep use this Mac, where its
/// link stands, and the two permissions desktop control needs.
function thisMac(mac: MacControl) {
  const p = mac.permissions;
  const sub = !mac.enabled ? "Beep can see the screen, click, type and run commands here when you ask"
    : mac.status === "failed" ? `Couldn't connect: ${esc(mac.error || "unknown error")}`
    : mac.status === "connected" ? (p.screen_recording && p.accessibility ? "Connected" : "Connected. Allow both below")
    : "Connecting…";
  const perm = (key: string, name: string, why: string, ok: boolean) => `<div class="dtoggle"><span><b>${name}</b><small>${why}</small></span>${ok
    ? '<span class="dtag ok">Allowed</span>'
    : `<button class="dbtn" data-allow="${key}">Allow…</button>`}</div>`;
  const rows = mac.enabled
    ? perm("ScreenRecording", "Screen Recording", "To see the screen", p.screen_recording)
      + perm("Accessibility", "Accessibility", "To click, type and read app controls", p.accessibility)
    : "";
  return `<div class="dsec flat top">This Mac</div>
    <div class="dtoggle"><span><b>Let Beep use this Mac</b><small>${sub}</small></span>
      ${switchHtml(mac.enabled, "Let Beep use this Mac", `id="usemac"`)}</div>${rows}`;
}

/// Settings: what a new notice does while you are at this Mac.
const VOICE_NOTES: [NoticeVoice, string][] = [["ask", "Ask first"], ["aloud", "Read aloud right away"], ["off", "Off"]];
const VOICE_NOTES_SAY: Record<NoticeVoice, string> = {
  ask: "Shown by the orb with Play when you are here and not on a call",
  aloud: "Read aloud when you are here and not on a call",
  off: "Notification only",
};
function voiceNotesRow(cur: NoticeVoice) {
  return `<div class="dtoggle"><span><b>Voice notes</b><small>${VOICE_NOTES_SAY[cur] ?? ""}</small></span></div>
    <div class="dchips dgap" role="radiogroup" aria-label="Voice notes">${VOICE_NOTES.map(([k, label]) =>
      `<button class="dchip" role="radio" aria-checked="${cur === k}" aria-pressed="${cur === k}" data-vn="${k}">${label}</button>`).join("")}</div>`;
}

/// Settings, General: whether coding agents notify when they ask or finish.
function agentNoticesRow(on: boolean) {
  const why = on ? "When a coding agent asks something or finishes a turn" : "Coding agents only show under Waiting";
  return `<div class="dtoggle"><span><b>Coding agent notifications</b><small>${why}</small></span>
    ${switchHtml(on, "Coding agent notifications", `id="agentNotices"`)}</div>`;
}

/// Settings, under Voice notes: what ends a spoken message in live mode.
function sendRow(onPause: boolean) {
  const why = onPause ? "Sends after a short pause. Saying “send” sends at once." : "Pauses never send. Say “send” or “stop” when you are done.";
  return `<div class="dtoggle"><span><b>When I stop talking</b><small>${why}</small></span></div>
    <div class="dchips dgap" role="radiogroup" aria-label="When I stop talking"><button class="dchip" role="radio" aria-checked="${onPause}" aria-pressed="${onPause}" data-pause="1">Send when I stop talking</button><button class="dchip" role="radio" aria-checked="${!onPause}" aria-pressed="${!onPause}" data-pause="0">Wait until I say “send”</button></div>`;
}

/// Settings: drawn at once from what this Mac knows (its own settings and the
/// last devices, apps and machines seen), then each server-backed section fills
/// in on its own as its call answers.
export const SET: {
  root: HTMLElement | null;
  token: number;
  devices: Device[] | null;
  machines: Machine[] | null | undefined;
  confirm: boolean;
  modelsError?: boolean;
  path: SettingsPath | null;
  local: { set: MacSettings; mac: MacControl | null; linked: boolean } | null;
  unembed: (() => void) | null;
} = { root: null, token: 0, devices: null, machines: undefined, confirm: false, path: null, local: null, unembed: null };

/// Changes a model setting from Settings and redraws the section with
/// what the server answers.
async function setModelChange(command: string, args: Record<string, unknown>) {
  try {
    MODELS.info = await invoke<Models>(command, args);
  } catch (e) {
    const err = setRoot().querySelector("#setModelErr");
    if (err) err.textContent = String(e);
    return;
  }
  refreshHints();
  if (setShown()) fillSection("models", setModelsHtml());
}

export const setRoot = () => SET.root ?? bar;
/// The sheet is open, or the bar still shows Settings.
export const setShown = () => SET.root ? SET.root.isConnected : S.view === "settings";

// Permissions are granted in System Settings; coming back shows them.
// Not while a token is typed in Credentials: the redraw would drop it.
// In the window the fields are in the sheet, not the pane.
window.addEventListener("focus", () => {
  const $set = (sel: string) => setRoot().querySelector<HTMLInputElement>(sel);
  if (setShown() && !$set("#unlinkYes") && !$set("#credSecret")?.value && !$set("#agSecret")?.value) renderSettings();
});

/// Rows that stand in for a section until the server answers.
export const setSkeleton = (n: number) => `<div class="dskel-rows">${[78, 56, 68, 48].slice(0, n).map(w => `<div class="dskel" style="width:${w}%"></div>`).join("")}</div>`;

function setDevicesHtml(linked: boolean, set: MacSettings) {
  if (!linked) return `<div class="dtoggle"><span><b>Subscription</b><small>${SUBSCRIPTION_NOTE}</small></span><button class="dbtn pri" id="subscribe">Sign in</button></div><div id="subNote"></div>`;
  if (SET.devices === null) return setSkeleton(2);
  const devices = SET.devices;
  // Whether this Mac made the account, and so links and removes devices.
  const owner = devices.some(d => d.this_device && d.primary);
  const row = (d: Device) => {
    const tags = [d.this_device ? "This Mac" : "", d.primary ? "Owner" : ""].filter(Boolean)
      .map(t => `<span class="dtag">${t}</span>`).join("");
    const remove = owner && !d.this_device ? `<button class="dbtn quiet" data-remove="${esc(d.id)}" aria-label="Remove ${esc(d.name)}">${ICON.x}</button>` : "";
    return `<div class="dtoggle"><span><b class="dline tight">${esc(d.name)}${tags}</b><small>${d.kind === "phone" ? "Phone" : "Computer"}</small></span>${remove}</div>`;
  };
  const ask = SET.confirm ? `<div class="dcard"><div class="dask">
      <b class="dtitle">Unlink this Mac?</b><div class="dmuted dblock">This Mac stops seeing your chats and memories. Your phone keeps them. To use Beep here again, start over and link it from the phone.</div>
      <div class="dline"><button class="dbtn warn" id="unlinkYes">Unlink</button><button class="dbtn quiet" id="unlinkNo">Cancel</button></div>
      <div class="derr" id="unlinkErr"></div></div></div>` : "";
  return devices.map(row).join("") + (owner
    ? '<div class="dtoggle"><span><small>Scan its code from Beep on the phone to bring its chats here</small></span><button class="dbtn pri" id="invite">Link a phone…</button></div>'
    : SET.confirm ? "" : `<div class="dtoggle"><span><small>This Mac: ${esc(set.name)}</small></span><button class="dbtn" id="invite">Link a phone…</button><button class="dbtn warn" id="unlink">Unlink this Mac</button></div>`) + ask;
}

/// Settings, Models: the model every turn of yours uses, how long it thinks,
/// and Auto.
function setModelsHtml() {
  const m = MODELS.info;
  if (!m) return SET.modelsError ? `<div class="dmuted">Could not load your models.</div>` : setSkeleton(2);
  const chosen = m.choices.find((c: ModelChoice) => c.id === m.selected);
  const options = m.choices.map((c: ModelChoice) => `<option value="${esc(c.id)}"${c.id === m.selected ? " selected" : ""}>${esc(c.name)}</option>`).join("");
  const auto = m.auto_available
    ? `<div class="dtoggle"><span><b>Auto thinking</b><small>${m.auto ? "Beep picks the thinking for each prompt" : "Every prompt thinks as set below"}</small></span>
        ${switchHtml(m.auto, "Auto thinking", `id="setAuto"`)}</div>`
    : "";
  const levels = THINKING.map(l => `<button class="dchip" data-setlevel="${l}" aria-pressed="${!m.auto && l === m.thinking}"${m.auto ? " disabled" : ""}>${cap(l)}</button>`).join("");
  return `<div class="dtoggle"><span><b>Model</b><small>${chosen ? esc(modelPrice(chosen)) : "Your turns use the server's default"}</small></span>
      <select id="setModel" aria-label="Model">${options}</select></div>
    ${auto}
    <div class="dfield"><span>Thinking${m.auto ? " (Auto picks it per prompt)" : ""}</span><div class="dchips">${levels}</div></div>
    <div class="derr" id="setModelErr"></div>`;
}

/// Settings, Account: who this account signs in to GitHub as, when known,
/// above its devices.
export function setAccountHtml(linked: boolean) {
  if (!linked) return "";
  const gh = Array.isArray(CRED.github) ? CRED.github.find(a => a.default) : null;
  return gh ? `<div class="dtoggle"><span><b>GitHub</b><small>@${esc(gh.login)}, given to your machines as GH_TOKEN</small></span></div>` : "";
}

function setAppsHtml() {
  if (!A.apps) return setSkeleton(1);
  const on = A.apps.filter((a: App) => a.connected).map((a: App) => a.name);
  return `<div class="dtoggle"><span><b>Connected apps</b><small>${on.length ? `${on.length} connected: ${esc(on.join(", "))}` : "Gmail, Calendar, Notion and more"}</small></span><button class="dbtn" id="apps">Manage…</button></div>`;
}

/// Other machines; left out when the list could not be read.
function setMachinesHtml() {
  if (SET.machines === undefined) return setSkeleton(1);
  const list = SET.machines;
  if (!list) return "";
  return `<div class="dtoggle"><span><b>Other machines</b><small>${list.length ? esc(`${list.length} linked: ${list.map(m => m.name).join(", ")}`) : "None linked yet"}</small></span><button class="dbtn" id="machines">Show…</button></div>`;
}

/// Leaves Settings for another page: in the window the sheet closes and
/// the pane shows it.
function setAway(view: string, inBar: () => unknown) {
  if (SET.root && windowHooks) return windowHooks.settingsAway(view);
  inBar();
}

/// Runs a change Settings asks this Mac or the server for, then draws
/// Settings again from what is true now. A failure only goes to the console.
async function thenRedraw(change: Promise<unknown>) {
  try {
    await change;
  } catch (e) {
    console.warn(e);
  }
  if (setShown()) renderSettings();
}

/// Binds the controls inside `el`, a section or the whole page.
function bindSettings(el: Element) {
  const $in = <T extends Element = HTMLElement>(sel: string) => el.querySelector<T>(sel);
  const each = (attr: string, fn: (b: HTMLButtonElement) => void) => el.querySelectorAll<HTMLButtonElement>(`[${attr}]`).forEach(fn);
  /// A switch that waits, disabled, for `command` to flip it, then redraws.
  const flip = (sel: string, command: string) => $in(sel)?.addEventListener("click", e => {
    const sw = e.currentTarget as HTMLButtonElement;
    sw.disabled = true;
    thenRedraw(invoke(command, { enabled: sw.getAttribute("aria-checked") !== "true" }));
  });
  $in("#subscribe")?.addEventListener("click", e => signIn(el, e.currentTarget as HTMLButtonElement));
  $in("#invite")?.addEventListener("click", () => setAway("invite", renderInvitePhone));
  $in("#apps")?.addEventListener("click", () => setAway("apps", renderApps));
  $in("#machines")?.addEventListener("click", () => setAway("machines", renderMachines));
  // Instructions is a view of the window: the sheet closes onto it, and
  // the bar hands it over.
  $in("#openInstr")?.addEventListener("click", () => (windowHooks ? windowHooks.openInstructions() : toWindow("instructions")));
  $in("#openProviders")?.addEventListener("click", () => (windowHooks ? windowHooks.openProviders() : toWindow("providers")));
  each("data-remove", b => b.addEventListener("click", () => {
    b.disabled = true;
    thenRedraw(invoke("remove_device", { id: b.dataset.remove }));
  }));
  each("data-skin-pick", b => b.addEventListener("click", () => {
    setSkin(b.dataset.skinPick!);
    for (const x of el.querySelectorAll("[data-skin-pick]")) x.setAttribute("aria-pressed", String(x === b));
  }));
  $in("#setModel")?.addEventListener("change", e => setModelChange("set_model", { id: (e.target as HTMLSelectElement).value }));
  $in("#setAuto")?.addEventListener("click", () => setModelChange("set_auto_model", { enabled: !MODELS.info!.auto }));
  each("data-setlevel", b => b.addEventListener("click", () => setModelChange("set_thinking", { level: b.dataset.setlevel })));
  $in("#unlink")?.addEventListener("click", () => renderSettings(true));
  $in("#unlinkNo")?.addEventListener("click", () => renderSettings());
  $in("#unlinkYes")?.addEventListener("click", async () => {
    try {
      await invoke("unlink");
      if (SET.root) windowHooks?.settingsAway(null);
      show(null);
    } catch (e) {
      // In the window it is the sheet, not the pane, that holds the error.
      const err = setRoot().querySelector("#unlinkErr");
      if (err) err.textContent = String(e);
    }
  });
  flip("#usemac", "set_mac_control");
  each("data-allow", b => b.addEventListener("click", () =>
    invoke("allow_mac_permission", { permission: b.dataset.allow }).catch(e => console.warn(e))));
  each("data-vn", b => b.addEventListener("click", () => thenRedraw(invoke("set_notice_voice", { choice: b.dataset.vn }))));
  each("data-pause", b => b.addEventListener("click", () => thenRedraw(invoke("set_send_on_pause", { enabled: b.dataset.pause === "1" }))));
  flip("#agentNotices", "set_agent_notices");
  bindCreds(el);
  bindAgents(el);
  $in("#autostart")?.addEventListener("click", async e => {
    const sw = e.currentTarget as HTMLElement, on = sw.getAttribute("aria-checked") !== "true";
    try {
      await invoke("set_autostart", { enabled: on });
      sw.setAttribute("aria-checked", String(on));
    } catch (err) {
      console.warn(err);
    }
  });
}

/// Signs in to a subscription: in the browser on a hosted server, with a token to paste if the
/// link did not open; else the account starts on this Mac. Settings redraws when the link comes.
async function signIn(el: Element, button: HTMLButtonElement) {
  const note = el.querySelector<HTMLElement>("#subNote");
  button.disabled = true;
  try {
    if (signInWay(await invoke<boolean>("login_hosted")) === "here") return void await invoke("use_here");
    await invoke("start_login");
    if (!note) return;
    note.innerHTML = `<label class="dfield">Finish in your browser, or paste the token<input id="subToken" class="mono" autocomplete="off" spellcheck="false" placeholder="Token"></label><div class="derr" id="subErr"></div>`;
    const token = note.querySelector<HTMLInputElement>("#subToken")!;
    token.addEventListener("input", () => {
      const text = token.value.trim();
      if (!/^[\w-]+\.[\w-]+$/.test(text)) return;
      token.disabled = true;
      invoke("complete_login", { token: text }).catch(err => { token.disabled = false; note.querySelector("#subErr")!.textContent = String(err); });
    });
    token.focus();
  } catch (err) {
    if (note) note.innerHTML = `<div class="derr">${esc(err)}</div>`;
  } finally {
    button.disabled = false;
  }
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

/// The skin picker: a swatch per skin, dark ones then light ones.
function skinsHtml() {
  const on = skinId();
  const group = (title: string, light: boolean) => `<div class="dskins"><small>${title}</small><div>${SKINS.filter(k => !!k.light === light).map(k =>
    `<button class="dskin" data-skin-pick="${k.id}" aria-pressed="${k.id === on}"><i style="background:${k.swatch}"></i>${esc(k.name)}</button>`).join("")}</div></div>`;
  return group("Dark", false) + group("Light", true);
}

/// The sections of Settings, in order, each with the page it is.
function settingsSections(set: MacSettings, mac: MacControl | null, linked: boolean, sheet: boolean): [SettingsPage, string][] {
  const sec = (id: string, title: string, body: string) => `<section id="set-${id}-sec" data-sec="${id}"><div class="dsec flat top">${title}</div><div id="set-${id}">${body}</div></section>`;
  // A page drawn by its own code, into this box.
  const host = (id: SettingsPage) => `<section id="set-${id}-sec" data-sec="${id}"><div class="dset-host" id="set-${id}-host"></div></section>`;
  const out: [SettingsPage, string][] = [["general", `<section id="set-general-sec" data-sec="general"><div class="dsec flat top">General</div>
      <div class="dtoggle"><span><b>Launch at login</b><small>Keep Beep in the menu bar after a restart</small></span>
        ${switchHtml(set.autostart, "Launch at login", `id="autostart"`)}</div>
      ${voiceNotesRow(set.notice_voice)}${sendRow(set.send_on_pause)}${agentNoticesRow(set.agent_notices)}</section>`]];
  out.push(["appearance", sec("appearance", "Appearance", skinsHtml())]);
  const machines: [SettingsPage, string] = ["machines", sheet ? host("machines") : sec("machines", "Machines", setMachinesHtml())];
  const creds: [SettingsPage, string] = ["creds", sec("creds", "Credentials", setCredsHtml())];
  const instr: [SettingsPage, string] = ["instr", sheet ? host("instr") : sec("instr", "Instructions", `<div class="dtoggle"><span><b>Your rules for every agent</b><small>One text for Claude Code, Codex, Gemini CLI and pi, with its versions, written into each agent's file on your machines</small></span><button class="dbtn" id="openInstr">Open…</button></div>`)];
  // Without a subscription these work over the mesh; the rest is the assistant's.
  if (!linked) return [...out, creds, machines, instr, ["devices", sec("devices", "Subscription", setDevicesHtml(linked, set))]];
  if (!MODELS.info || canPickModel()) out.push(["models", sec("models", "Models", setModelsHtml())]);
  out.push(["apps", sheet ? host("apps") : sec("apps", "Apps", setAppsHtml())]);
  if (mac) out.push(["mac", `<section id="set-mac-sec" data-sec="mac">${thisMac(mac)}</section>`]);
  out.push(machines);
  // The Coding agents rows sit outside #set-agents, which redraws alone.
  const sub = SET.path?.sub;
  out.push(["agents", sheet && sub?.kind === "agent" ? host("agents")
    : `<section id="set-agents-sec" data-sec="agents"><div class="dsec flat top">Agent accounts</div>${sheet ? "" : `<div class="dtoggle"><span><b>Coding agents on each machine</b><small>Versions and updates, on or off, binary paths, launch arguments and env vars for Claude Code, Codex, pi and the rest</small></span><button class="dbtn" id="openProviders">Open…</button></div>`}<div id="set-agents">${setAgentsHtml()}</div>${sheet ? codingAgentsHtml() : ""}</section>`]);
  out.push(creds, instr);
  out.push(["devices", sec("devices", "Account", `<div id="set-account">${setAccountHtml(linked)}</div>${setDevicesHtml(linked, set)}`)]);
  return out;
}

/// The coding agents Settings lists under Agent accounts, each opening its
/// versions, switch, binary path, launch arguments and env vars.
function codingAgentsHtml() {
  const row = (agent: string) => `<div class="dset-row" data-sub="${esc(subKey({ kind: "agent", id: agent }))}" role="button" tabindex="0">${harnessTile(agent)}
    <span class="tt"><b>${esc(harnessName(agent))}</b><small>Version and updates, on or off, binary path, launch arguments and env vars on each machine</small></span>${ICON.chev}</div>`;
  return `<div class="dset-h">Coding agents</div><div class="dset-box">${CODING_AGENTS.map(row).join("")}</div>`;
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
  const n = page === "apps" ? A.apps?.filter(a => a.connected).length : page === "machines" ? (SET.machines ?? MC.list)?.length : undefined;
  return n ? `<span class="n">${n}</span>` : "";
}

function navHtml(shown: SettingsPage[], path: SettingsPath) {
  return navEntries(shown, path, subEntries).map(({ page, sub, current }) => {
    const on = current ? ' aria-current="true"' : "";
    if (sub) return `<button class="cc-si sub" data-sub="${esc(subKey(sub))}"${on}><span class="tt">${esc(subLabel(sub))}</span></button>`;
    const label = page === "devices" && SET.local?.linked === false ? "Subscription" : pageLabel(page);
    return `<button class="cc-si" data-goto="${page}"${on}><span class="tt">${esc(label)}</span>${navCount(page)}</button>`;
  }).join("");
}

const BACK_ICON = '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"><path d="M15 6l-6 6 6 6"/></svg>';

/// The breadcrumb atop the right pane, with a back link on a subpage.
function crumbsHtml(path: SettingsPath) {
  const [page, sub] = crumbs(path, subLabel);
  if (sub === undefined) return `<b>${esc(page)}</b>`;
  return `<button class="dset-back" data-goto="${path.page}" aria-label="Back to ${esc(page)}">${BACK_ICON}</button>`
    + `<button class="dset-crumb" data-goto="${path.page}">${esc(page)}</button>${ICON.chev}<b>${esc(sub)}</b>`;
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

/// Draws the sheet: the left pane's pages, and the right pane with only the
/// page picked (or its subpage) under its breadcrumb.
function drawSheet() {
  const el = SET.root, local = SET.local;
  if (!el || !local) return;
  leavePage();
  const sections = settingsSections(local.set, local.mac, local.linked, true);
  const shown = sections.map(([id]) => id);
  const path = SET.path = fitPath(SET.path ?? recallPath(store()), shown, subExists);
  rememberPath(store(), path);
  // The page's sections are drawn from where the sub stands now.
  const page = settingsSections(local.set, local.mac, local.linked, true).find(([id]) => id === path.page)?.[1] ?? "";
  const focus = focusKey(el);
  el.innerHTML = `<nav class="dset-nav" aria-label="Settings pages"><b>Settings</b><small>Beep for macOS ${esc(local.set.version)}</small><div class="dset-pages">${navHtml(shown, path)}</div></nav>
    <div class="dset-body"><div class="dset-top"><span class="dset-crumbs">${crumbsHtml(path)}</span><kbd>esc</kbd></div>
    <div class="dset-page dform">${page}</div></div>`;
  bindNav(el);
  bindSettings(el.querySelector(".dset-page")!);
  mountPage(path);
  if (focus) refocus(el, focus);
}

/// Redraws the left pane and the breadcrumb only, as a page drawn by its
/// own code (Apps, Machines, a coding agent) opens another.
function drawChrome() {
  const el = SET.root, path = SET.path;
  if (!el?.isConnected || !path) return;
  const pages = el.querySelector<HTMLElement>(".dset-pages"), top = el.querySelector<HTMLElement>(".dset-crumbs");
  const focus = focusKey(el);
  if (pages) pages.innerHTML = navHtml(sheetPages(), path);
  if (top) top.innerHTML = crumbsHtml(path);
  bindNav(el);
  if (focus) refocus(el, focus);
}

/// The left pane, the breadcrumb and the rows that open a subpage.
function bindNav(el: HTMLElement) {
  for (const b of el.querySelectorAll<HTMLElement>("[data-goto]:not([data-bound]), [data-sub]:not([data-bound])")) {
    b.dataset.bound = "";
    const go = () => {
      const sub = parseSub(b.dataset.sub);
      if (sub) return goTo(openSub(sub));
      if (isPage(b.dataset.goto)) goTo(selectPage(b.dataset.goto));
    };
    b.addEventListener("click", go);
    if (b.tagName !== "BUTTON") b.addEventListener("keydown", e => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); go(); } });
  }
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
    SET.unembed = embed("instructions", box, undefined);
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
export async function renderSettings(confirm = false, root?: HTMLElement, page?: string) {
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
    const sections = settingsSections(set, mac, linked, false).map(([, html]) => html).join("");
    el.innerHTML = `<div class="dhead"><span class="tt"><b>Settings</b><small>Beep for macOS ${esc(set.version)}</small></span></div>
      <div class="dform dformcol">${sections}</div>${hints([["esc", "close"]])}`;
    bindSettings(el);
  }
  // The server's parts, side by side; each fills its section as it answers.
  if (linked) loadServerParts(set, alive);
  invoke<Machine[]>("list_machines").then(m => { SET.machines = m; MC.list = m; }, () => { SET.machines = null; })
    .then(() => {
      if (!alive()) return;
      if (SET.root) drawChrome(); else fillSection("machines", setMachinesHtml());
      // The git hosts say which machines have gh and teacli.
      if (CRED.list) fillSection("creds", setCredsHtml());
    });
  loadCreds();
  if (linked) loadAgentAccounts();
}

/// The subscription's parts of Settings, which fill their sections as each answers.
function loadServerParts(set: MacSettings, alive: () => boolean) {
  invoke<Device[]>("list_devices").then(d => { SET.devices = d; }, e => { console.warn(e); SET.devices ??= []; })
    .then(() => { if (alive()) fillSection("devices", `<div id="set-account">${setAccountHtml(true)}</div>${setDevicesHtml(true, set)}`); });
  invoke<Models>("get_models").then(m => { MODELS.info = m; SET.modelsError = false; }, e => { console.warn(e); SET.modelsError = !MODELS.info; })
    .then(() => {
      if (!alive()) return;
      // A server with no choices to offer leaves the section out.
      if (canPickModel() || SET.modelsError) return fillSection("models", setModelsHtml());
      if (SET.root && SET.path?.page === "models") return drawSheet();
      setRoot().querySelector("#set-models-sec")?.remove();
      setRoot().querySelector('[data-goto="models"]')?.remove();
    });
  invoke<App[]>("list_apps").then(a => { A.apps = a; }, () => { A.apps ??= []; })
    .then(() => {
      if (!alive()) return;
      if (SET.root) drawChrome(); else fillSection("apps", setAppsHtml());
    });
}

// Signing in changes which pages Settings has.
listen<string>("link-state", () => { if (setShown()) renderSettings(); });
