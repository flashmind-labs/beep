import { bindAgents, bindCreds, CRED, loadAgentAccounts, loadCreds, setAgentsHtml, setCredsHtml } from "./accounts.ts";
import { A, type App, renderApps } from "./apps.ts";
import { toWindow } from "./chat.ts";
import { $, bar, esc, ICON, invoke, S, windowHooks } from "./core.ts";
import { hints, refreshHints } from "./hints.ts";
import { renderInvitePhone, renderLink } from "./link.ts";
import { type Machine, MC, renderMachines } from "./machines.ts";
import { canPickModel, type ModelChoice, type Models, modelPrice, MODELS, THINKING } from "./model.ts";
import { cap } from "./tools.ts";
import { show } from "./views.ts";

// ---------- Settings ----------

/// What a new notice does while you are at this Mac (NoticeVoice).
export type NoticeVoice = "ask" | "aloud" | "off";

/// This Mac's own settings (`settings`).
interface MacSettings {
  /// The name this Mac links with.
  name: string;
  version: string;
  autostart: boolean;
  notice_voice: NoticeVoice;
  /// In live mode a pause sends, rather than only "send".
  send_on_pause: boolean;
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
    : mac.status === "failed" ? `Couldn't connect: ${esc(mac.error || "unknown error")}. Beep tries again on its own.`
    : mac.status === "connected" ? (p.screen_recording && p.accessibility ? "Connected. Ask Beep to do something on this Mac" : "Connected. Allow both below so Beep can see and use the screen")
    : "Connecting…";
  const perm = (key: string, name: string, why: string, ok: boolean) => `<div class="dtoggle"><span><b>${name}</b><small>${why}</small></span>${ok
    ? '<span class="dtag ok">Allowed</span>'
    : `<button class="dbtn" data-allow="${key}">Allow…</button>`}</div>`;
  const rows = mac.enabled
    ? perm("ScreenRecording", "Screen Recording", "To see the screen", p.screen_recording)
      + perm("Accessibility", "Accessibility", "To click, type and read app controls", p.accessibility)
    : "";
  return `<div class="dsec flat" style="padding-top:6px">This Mac</div>
    <div class="dtoggle"><span><b>Let Beep use this Mac</b><small>${sub}</small></span>
      <button class="dswitch" id="usemac" role="switch" aria-checked="${mac.enabled}" aria-label="Let Beep use this Mac"></button></div>${rows}`;
}

/// Settings: what a new notice does while you are at this Mac.
const VOICE_NOTES: [NoticeVoice, string][] = [["ask", "Ask first"], ["aloud", "Read aloud right away"], ["off", "Off"]];
const VOICE_NOTES_SAY: Record<NoticeVoice, string> = {
  ask: "New notices show by the orb with Play and Open when you are at this Mac and not on a call. Otherwise only a notification.",
  aloud: "New notices are read aloud when you are at this Mac and not on a call. Otherwise only a notification.",
  off: "New notices only show as a notification.",
};
function voiceNotesRow(cur: NoticeVoice) {
  return `<div class="dtoggle"><span><b>Voice notes</b><small>${VOICE_NOTES_SAY[cur] ?? ""}</small></span></div>
    <div class="dchips" role="radiogroup" aria-label="Voice notes" style="margin:6px 0 10px">${VOICE_NOTES.map(([k, label]) =>
      `<button class="dchip" role="radio" aria-checked="${cur === k}" aria-pressed="${cur === k}" data-vn="${k}">${label}</button>`).join("")}</div>`;
}

/// Settings, under Voice notes: what ends a spoken message in live mode.
function sendRow(onPause: boolean) {
  const why = onPause ? "Sends after a short pause. Saying “send” sends at once." : "Pauses never send. Say “send” or “stop” when you are done.";
  return `<div class="dtoggle"><span><b>When I stop talking</b><small>${why}</small></span></div>
    <div class="dchips" role="radiogroup" aria-label="When I stop talking" style="margin:6px 0 10px"><button class="dchip" role="radio" aria-checked="${onPause}" aria-pressed="${onPause}" data-pause="1">Send when I stop talking</button><button class="dchip" role="radio" aria-checked="${!onPause}" aria-pressed="${!onPause}" data-pause="0">Wait until I say “send”</button></div>`;
}

/// Settings: drawn at once from what this Mac knows (its own settings and
/// the last devices, apps and machines seen), then each server-backed
/// section fills in on its own as its call answers. The calls run side by
/// side: one after another they took as long as all four together. In the
/// bar it is the bar's page; in the window it is a sheet over the view
/// (`root`), which `settingsSheet` opens. `devices` is null until listed;
/// `machines` is undefined until listed and null when that failed;
/// `confirm` whether Unlink is asked; `token` names the latest drawing.
export const SET: {
  root: HTMLElement | null;
  token: number;
  devices: Device[] | null;
  machines: Machine[] | null | undefined;
  confirm: boolean;
  modelsError?: boolean;
} = { root: null, token: 0, devices: null, machines: undefined, confirm: false };
const SET_SECTIONS = [["general", "General"], ["models", "Models"], ["apps", "Apps"], ["mac", "This Mac"], ["machines", "Machines"], ["agents", "Agent accounts"], ["creds", "Credentials"], ["instr", "Instructions"], ["devices", "Account"]];

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
window.addEventListener("focus", () => {
  const typed = (sel: string) => $<HTMLInputElement>(sel)?.value;
  if (setShown() && !$("#unlinkYes") && !typed("#credSecret") && !typed("#agSecret")) renderSettings();
});

/// Rows that stand in for a section until the server answers.
export const setSkeleton = (n: number) => `<div class="dskel-rows">${[78, 56, 68, 48].slice(0, n).map(w => `<div class="dskel" style="width:${w}%"></div>`).join("")}</div>`;

function setDevicesHtml(linked: boolean, set: MacSettings) {
  if (!linked) return `<div class="dtoggle"><span><b>Not linked</b><small>Link to see your chats and memories here</small></span><button class="dbtn pri" id="link">Link to phone…</button></div>`;
  if (SET.devices === null) return setSkeleton(2);
  const devices = SET.devices;
  // Whether this Mac made the account, and so links and removes devices.
  const owner = devices.some(d => d.this_device && d.primary);
  const row = (d: Device) => {
    const tags = [d.this_device ? "This Mac" : "", d.primary ? "Owner" : ""].filter(Boolean)
      .map(t => `<span class="dtag">${t}</span>`).join("");
    const remove = owner && !d.this_device ? `<button class="dbtn quiet" data-remove="${esc(d.id)}" aria-label="Remove ${esc(d.name)}">${ICON.x}</button>` : "";
    return `<div class="dtoggle"><span><b style="display:flex;gap:7px;align-items:center">${esc(d.name)}${tags}</b><small>${d.kind === "phone" ? "Phone" : "Computer"}</small></span>${remove}</div>`;
  };
  const ask = SET.confirm ? `<div class="dcard"><div style="padding:10px 12px;display:flex;flex-direction:column;gap:8px">
      <b style="font-size:13.5px">Unlink this Mac?</b><div class="dmuted" style="display:block">This Mac stops seeing your chats and memories. Your phone keeps them. To use Beep here again, start over and link it from the phone.</div>
      <div style="display:flex;gap:8px"><button class="dbtn warn" id="unlinkYes">Unlink</button><button class="dbtn" id="unlinkNo">Cancel</button></div>
      <div class="derr" id="unlinkErr"></div></div></div>` : "";
  return devices.map(row).join("") + (owner
    ? '<div class="dtoggle"><span><small>Scan its code from Beep on the phone to bring its chats here</small></span><button class="dbtn pri" id="invite">Link a phone…</button></div>'
    : SET.confirm ? "" : `<div class="dtoggle"><span><small>This Mac: ${esc(set.name)}</small></span><button class="dbtn" id="invite">Link a phone…</button><button class="dbtn warn" id="unlink">Unlink this Mac</button></div>`) + ask;
}

/// Settings, Models: the model every turn of yours uses, how long it
/// thinks, and Auto. Auto picks only the thinking for each prompt; the
/// model stays the one picked here.
function setModelsHtml() {
  const m = MODELS.info;
  if (!m) return SET.modelsError ? `<div class="dmuted">Could not load your models.</div>` : setSkeleton(2);
  const chosen = m.choices.find((c: ModelChoice) => c.id === m.selected);
  const options = m.choices.map((c: ModelChoice) => `<option value="${esc(c.id)}"${c.id === m.selected ? " selected" : ""}>${esc(c.name)}</option>`).join("");
  const auto = m.auto_available
    ? `<div class="dtoggle"><span><b>Auto thinking</b><small>${m.auto ? "On: Beep picks how long to think for each prompt, on your model" : "Off: every prompt thinks as set below"}</small></span>
        <button class="dswitch" id="setAuto" role="switch" aria-checked="${m.auto}" aria-label="Auto thinking"></button></div>`
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
  $in("#link")?.addEventListener("click", () => setAway("link", renderLink));
  $in("#invite")?.addEventListener("click", () => setAway("invite", renderInvitePhone));
  $in("#apps")?.addEventListener("click", () => setAway("apps", renderApps));
  $in("#machines")?.addEventListener("click", () => setAway("machines", renderMachines));
  // Instructions is a view of the window: the sheet closes onto it, and
  // the bar hands it over.
  $in("#openInstr")?.addEventListener("click", () => (windowHooks ? windowHooks.openInstructions() : toWindow("instructions")));
  each("data-remove", b => b.addEventListener("click", () => {
    b.disabled = true;
    thenRedraw(invoke("remove_device", { id: b.dataset.remove }));
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
  $in("#usemac")?.addEventListener("click", e => {
    const sw = e.currentTarget as HTMLButtonElement;
    sw.disabled = true;
    thenRedraw(invoke("set_mac_control", { enabled: sw.getAttribute("aria-checked") !== "true" }));
  });
  each("data-allow", b => b.addEventListener("click", () =>
    invoke("allow_mac_permission", { permission: b.dataset.allow }).catch(e => console.warn(e))));
  each("data-vn", b => b.addEventListener("click", () => thenRedraw(invoke("set_notice_voice", { choice: b.dataset.vn }))));
  each("data-pause", b => b.addEventListener("click", () => thenRedraw(invoke("set_send_on_pause", { enabled: b.dataset.pause === "1" }))));
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
const FOCUS_KEYS = ["remove", "vn", "pause", "allow", "goto", "setlevel"];

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

/// Draws Settings into `root` (the window's sheet), or the bar when none.
export async function renderSettings(confirm = false, root?: HTMLElement) {
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
  const sec = (id: string, title: string, body: string) => `<section id="set-${id}-sec" data-sec="${id}"><div class="dsec flat" style="padding-top:6px">${title}</div><div id="set-${id}">${body}</div></section>`;
  const sections = `<section id="set-general-sec" data-sec="general"><div class="dsec flat" style="padding-top:6px">General</div>
      <div class="dtoggle"><span><b>Launch at login</b><small>Keep Beep in the menu bar after a restart</small></span>
        <button class="dswitch" id="autostart" role="switch" aria-checked="${set.autostart}" aria-label="Launch at login"></button></div>
      ${voiceNotesRow(set.notice_voice)}${sendRow(set.send_on_pause)}</section>`
    + (linked && (!MODELS.info || canPickModel()) ? sec("models", "Models", setModelsHtml()) : "")
    + (linked ? sec("apps", "Apps", setAppsHtml()) : "")
    + (linked && mac ? `<section id="set-mac-sec" data-sec="mac">${thisMac(mac)}</section>` : "")
    + (linked ? sec("machines", "Machines", setMachinesHtml()) : "")
    + (linked ? sec("agents", "Agent accounts", setAgentsHtml()) : "")
    + (linked ? sec("creds", "Credentials", setCredsHtml()) : "")
    + (linked ? sec("instr", "Instructions", `<div class="dtoggle"><span><b>Your rules for every agent</b><small>One text for Claude Code, Codex, Gemini CLI and pi, with its versions, written into each agent's file on your machines</small></span><button class="dbtn" id="openInstr">Open…</button></div>`) : "")
    + sec("devices", "Account", `<div id="set-account">${setAccountHtml(linked)}</div>${setDevicesHtml(linked, set)}`);
  if (SET.root) {
    const shown = new Set([...sections.matchAll(/data-sec="(\w+)"/g)].map(m => m[1]));
    const nav = SET_SECTIONS.filter(([id]) => shown.has(id))
      .map(([id, label], i) => `<button class="cc-si" data-goto="${id}"${i === 0 ? ' aria-current="true"' : ""}>${label}</button>`).join("");
    const top = el.querySelector(".dset-body")?.scrollTop ?? 0;
    const focus = focusKey(el);
    el.innerHTML = `<nav class="dset-nav" aria-label="Settings sections"><b>Settings</b>${nav}</nav>
      <div class="dset-body"><div class="dset-top"><span class="tt"><b>Settings</b><small>Beep for macOS ${esc(set.version)}</small></span><kbd>esc</kbd></div>
      <div class="dform" style="display:flex;flex-direction:column">${sections}</div></div>`;
    el.querySelector(".dset-body")!.scrollTop = top;
    el.querySelectorAll<HTMLElement>("[data-goto]").forEach(b => b.addEventListener("click", () => {
      el.querySelectorAll("[data-goto]").forEach(x => x.removeAttribute("aria-current"));
      b.setAttribute("aria-current", "true");
      el.querySelector(`#set-${b.dataset.goto}-sec`)?.scrollIntoView({ block: "start", behavior: "smooth" });
    }));
    bindSettings(el);
    if (focus) refocus(el, focus);
  } else {
    el.innerHTML = `<div class="dhead"><span class="tt"><b>Settings</b><small>Beep for macOS ${esc(set.version)}</small></span></div>
      <div class="dform" style="padding-top:12px;display:flex;flex-direction:column">${sections}</div>${hints([["esc", "close"]])}`;
    bindSettings(el);
  }
  if (!linked) return;
  // The server's parts, side by side; each fills its section as it answers.
  invoke<Device[]>("list_devices").then(d => { SET.devices = d; }, e => { console.warn(e); SET.devices ??= []; })
    .then(() => { if (alive()) fillSection("devices", `<div id="set-account">${setAccountHtml(true)}</div>${setDevicesHtml(true, set)}`); });
  invoke<Models>("get_models").then(m => { MODELS.info = m; SET.modelsError = false; }, e => { console.warn(e); SET.modelsError = !MODELS.info; })
    .then(() => {
      if (!alive()) return;
      // A server with no choices to offer leaves the section out.
      if (canPickModel() || SET.modelsError) return fillSection("models", setModelsHtml());
      setRoot().querySelector("#set-models-sec")?.remove();
      setRoot().querySelector('[data-goto="models"]')?.remove();
    });
  invoke<App[]>("list_apps").then(a => { A.apps = a; }, () => { A.apps ??= []; })
    .then(() => { if (alive()) fillSection("apps", setAppsHtml()); });
  invoke<Machine[]>("list_machines").then(m => { SET.machines = m; MC.list = m; }, () => { SET.machines = null; })
    .then(() => {
      if (!alive()) return;
      fillSection("machines", setMachinesHtml());
      // The git hosts say which machines have gh and teacli.
      if (CRED.list) fillSection("creds", setCredsHtml());
    });
  loadCreds();
  loadAgentAccounts();
}
