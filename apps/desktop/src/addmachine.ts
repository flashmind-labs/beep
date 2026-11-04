// The "Add a machine" sheet.
import { invoke } from "./tauri.ts";
import { INSTALL, START, next, timeLeft, machineById, describeMachine, macOption } from "./addmachine-state.ts";
import type { AddEvent, MacSeen } from "./addmachine-state.ts";
import type { MacControl, MachineView } from "./home.ts";
import { button, el, present } from "./projects-state.ts";

/// A setup token as `create_node_token` gives it (TokenView).
interface NodeToken {
  token: string;
  command: string;
  /// In milliseconds since the Unix epoch.
  expires_at: number;
}

/// What add_machine_over_ssh gives (SshAdd).
interface SshAdd {
  machine_id: string | null;
  system: string;
  error: string;
  output: string[];
}

type Timer = ReturnType<typeof setInterval> | undefined;

const POLL_MS = 2000;
const TICK_MS = 1000;

let root: HTMLDivElement | null = null, sheet: HTMLDivElement | null = null, state = START;
let ticker: Timer, poller: Timer, back: Element | null = null, onClose: (() => void) | null = null;
// This Mac's mac_control view, and whether the sheet turned it on.
let mac: MacSeen | null = null, macAsked = false, macPoller: Timer;
// Over SSH: the online machines that can run ssh, the one picked, its
// hosts (null while loading), the target being added and the last failure.
let sshMachines: MachineView[] = [], sshFrom = "", sshHosts: string[] | null = null;
let sshBusy: string | null = null, sshFailed: { target: string; add: SshAdd } | null = null;
let sshInput: HTMLInputElement | null = null;

/// A dialog over the window: the dimmed scrim holding the sheet, not yet
/// on the page.
export function sheetShell(label: string, cls: string) {
  const scrim = el("div", "am-scrim");
  const box = el("div", cls);
  box.setAttribute("role", "dialog");
  box.setAttribute("aria-modal", "true");
  box.setAttribute("aria-label", label);
  scrim.append(box);
  return { root: scrim, sheet: box };
}

/// Gives focus back to what had it before a sheet opened.
export function focusBack(was: Element | null) {
  const f = was as HTMLElement | null;
  if (f && f.focus) f.focus();
}

/// Loads the sheets' shared styles once, so the window only has to import
/// the sheets.
export function linkSheetStyle() {
  if (document.querySelector("link[data-am]")) return;
  const link = document.createElement("link");
  link.rel = "stylesheet";
  link.href = new URL("./addmachine.css", import.meta.url).href;
  link.dataset.am = "";
  document.head.append(link);
}

function copyButton(text: string) {
  const b = button("Copy", "dbtn", async () => {
    try {
      await navigator.clipboard.writeText(text);
      b.textContent = "Copied";
    } catch {
      b.textContent = "Select and copy";
    }
    setTimeout(() => { if (b.isConnected) b.textContent = "Copy"; }, 1500);
  });
  return b;
}

function codeLine(text: string) {
  return el("div", "am-line", el("div", "am-code", text), copyButton(text));
}

/// Opens the sheet over the window. `options.onClose` runs once it closes,
/// as when the window wants to refresh its machines.
export function openAddMachine(options: { onClose?: () => void } = {}) {
  if (root) return;
  onClose = options.onClose || null;
  back = document.activeElement;
  linkSheetStyle();
  ({ root, sheet } = sheetShell("Add a machine", "am-sheet"));
  document.body.append(root);
  document.addEventListener("keydown", onKey, true);
  ticker = setInterval(() => update({ type: "tick", now: Date.now() }), TICK_MS);
  mac = null;
  macAsked = false;
  loadMac();
  macPoller = setInterval(loadMac, POLL_MS);
  sshMachines = [];
  sshFrom = "";
  sshHosts = null;
  sshBusy = null;
  sshFailed = null;
  sshInput = el("input", "am-code am-target");
  sshInput.placeholder = "user@host";
  sshInput.setAttribute("aria-label", "SSH target");
  sshInput.spellcheck = false;
  sshInput.addEventListener("keydown", e => {
    if (e.key === "Enter" && sshInput) addOverSsh(sshInput.value);
  });
  loadSsh();
  start();
}

/// The online machines, and the hosts the default one's ssh knows: this
/// Mac's when it is online, else the first.
async function loadSsh() {
  try {
    const [machines, me] = await Promise.all([
      invoke<MachineView[]>("list_machines"),
      invoke<MacControl>("mac_control").catch(() => null),
    ]);
    if (!root) return;
    sshMachines = machines.filter(m => m.online);
    sshFrom = (sshMachines.find(m => m.id === me?.id) || sshMachines[0] || { id: "" }).id;
  } catch (e) {
    console.warn("machines for ssh:", e);
    return;
  }
  loadHosts();
}

async function loadHosts() {
  const from = sshFrom;
  sshHosts = null;
  drawSsh();
  if (!from) return;
  let hosts: string[] = [];
  try {
    hosts = await invoke<string[]>("ssh_hosts", { machineId: from });
  } catch (e) {
    console.warn("ssh hosts:", e);
  }
  if (root && from === sshFrom) {
    sshHosts = hosts;
    drawSsh();
  }
}

/// Has the picked machine install beep-node on `target` over ssh; the sheet
/// shows Linked once it is online.
async function addOverSsh(target: string) {
  target = target.trim();
  if (!target || sshBusy || !sshFrom) return;
  sshBusy = target;
  sshFailed = null;
  drawSsh();
  let add: SshAdd;
  try {
    add = await invoke<SshAdd>("add_machine_over_ssh", { machineId: sshFrom, target });
    if (add.machine_id) {
      const machine = machineById(await invoke<MachineView[]>("list_machines"), add.machine_id);
      if (machine && root) {
        sshBusy = null;
        update({ type: "found", machine });
        return;
      }
    }
  } catch (e) {
    add = { machine_id: null, system: "", error: String(e), output: [] };
  }
  sshBusy = null;
  if (!add.machine_id) sshFailed = { target, add };
  if (root) drawSsh();
}

/// Closes the sheet, revoking its token unless the machine used it.
export function closeAddMachine() {
  if (!root) return;
  if (state.step === "waiting") invoke("revoke_node_token", { token: state.token }).catch(() => {});
  clearInterval(ticker);
  clearInterval(poller);
  clearInterval(macPoller);
  ticker = poller = macPoller = undefined;
  document.removeEventListener("keydown", onKey, true);
  root.remove();
  root = sheet = null;
  state = START;
  focusBack(back);
  const done = onClose;
  onClose = null;
  if (done) done();
}

function onKey(e: KeyboardEvent) {
  if (e.key !== "Escape") return;
  e.preventDefault();
  e.stopPropagation();
  closeAddMachine();
}

/// Asks for a token.
async function start() {
  update({ type: "renew" });
  clearInterval(poller);
  poller = undefined;
  try {
    const made = await invoke<NodeToken>("create_node_token", { name: null });
    if (!root) {
      invoke("revoke_node_token", { token: made.token }).catch(() => {});
      return;
    }
    update({ type: "created", token: made.token, command: made.command, expiresAt: made.expires_at });
    poller = setInterval(poll, POLL_MS);
  } catch (e) {
    update({ type: "failed", message: String(e) });
  }
}

async function poll() {
  if (state.step !== "waiting") return;
  const token = state.token;
  try {
    const id = await invoke<string | null>("node_token_machine", { token });
    if (!id) return;
    // Matched by id, so a machine set up again is found too.
    const machine = machineById(await invoke<MachineView[]>("list_machines"), id);
    if (machine && state.step === "waiting" && state.token === token) update({ type: "found", machine });
  } catch {
    // The next poll tries again.
  }
}

/// Reads where this Mac stands and redraws its option.
async function loadMac() {
  try {
    const view = await invoke<MacControl>("mac_control");
    if (!root) return;
    mac = view;
  } catch (e) {
    console.warn("this Mac's state:", e);
    return;
  }
  drawMac();
}

/// Turns on Let Beep use this Mac.
async function useThisMac() {
  macAsked = true;
  try {
    mac = await invoke<MacControl>("set_mac_control", { enabled: true });
  } catch (e) {
    mac = { ...mac, enabled: true, status: "failed", error: String(e) };
  }
  if (root) drawMac();
}

/// Redraws only the Mac option, so the token lines keep their state.
function drawMac() {
  if (!sheet) return;
  const was = sheet.querySelector(".am-mac");
  const now = macBlock();
  if (was && now) was.replaceWith(now);
  else if (!!was !== !!now) draw();
}

/// Redraws only the SSH part and the status line, so the typed target and
/// the token lines keep their state.
function drawSsh() {
  if (!sheet || state.step !== "waiting") return;
  const block = sheet.querySelector(".am-ssh"), now = sshBlock();
  if (block && now) block.replaceWith(now);
  else if (!!block !== !!now) return draw();
  const status = sheet.querySelector(".am-status");
  if (status) status.replaceWith(statusLine());
}

/// Over SSH: which machine runs ssh, its known hosts, and a target field.
function sshBlock() {
  if (!sshMachines.length || !sshInput) return null;
  const pick = el("select", "am-from");
  pick.setAttribute("aria-label", "Machine that runs ssh");
  for (const m of sshMachines) {
    const option = el("option", "", m.name);
    option.value = m.id;
    option.selected = m.id === sshFrom;
    pick.append(option);
  }
  pick.disabled = !!sshBusy;
  pick.addEventListener("change", () => {
    sshFrom = pick.value;
    sshFailed = null;
    loadHosts();
  });
  const from = sshMachines.find(m => m.id === sshFrom);
  const hosts = sshHosts === null ? null
    : sshHosts.length ? el("div", "am-hosts", ...sshHosts.map(host => {
      const b = button(host, "dbtn", () => addOverSsh(host));
      b.disabled = !!sshBusy;
      if (host === sshBusy) b.setAttribute("aria-pressed", "true");
      return b;
    }))
    : el("div", "am-label", `No hosts in ~/.ssh/config or known_hosts on ${from ? from.name : "it"}.`);
  sshInput.disabled = !!sshBusy;
  return el("div", "am-mac am-ssh",
    el("div", "am-ssh-head", el("span", "am-other", "Over SSH"), el("span", "am-from-label", "from"), pick),
    hosts,
    sshInput);
}

/// Where the sheet says how it goes: an SSH add running or failed, else
/// the wait for the token.
function statusLine() {
  if (sshBusy) return el("div", "am-status am-wait", el("span", "dspin"), `Installing beep-node on ${sshBusy} over SSH`);
  if (sshFailed) {
    const { target, add } = sshFailed;
    return el("div", "am-status",
      el("div", "am-msg bad", `${target}${add.system ? ` (${add.system})` : ""}: ${add.error}`),
      add.output.length ? el("pre", "am-out", add.output.join("\n")) : null,
      el("div", "am-line", button("Try again", "dbtn pri", () => addOverSsh(target)), button("Close", "dbtn quiet", () => { sshFailed = null; drawSsh(); })));
  }
  if (state.step !== "waiting") return el("div", "am-status");
  return el("div", "am-status am-wait",
    el("span", "dspin"),
    "Waiting for the machine · token works once, ",
    el("span", "left", `${timeLeft(state.expiresAt, Date.now())} left`),
    el("span", "end", button("Cancel", "dbtn quiet", closeAddMachine)));
}

/// "Use this Mac" and where it stands, above the steps for other machines.
function macBlock() {
  if (state.step === "linked") return null;
  const option = macOption(mac, macAsked);
  if (!option) return null;
  const status = option.busy
    ? el("div", "am-wait", el("span", "dspin"), option.text)
    : el("div", `am-msg${option.bad ? " bad" : ""}`, option.done ? el("span", "ddot") : null, option.done ? " " : null, option.text);
  return el("div", "am-mac",
    el("div", "am-label", "This Mac"),
    option.button
      ? el("div", "am-line", el("div", "am-mac-text", option.text), button(option.button, "dbtn pri", useThisMac))
      : status);
}

function update(event: AddEvent) {
  const was = state;
  state = next(state, event);
  if (state.step !== "waiting") {
    clearInterval(poller);
    poller = undefined;
  }
  if (!root) return;
  // A tick only moves the countdown.
  if (state === was && event.type === "tick") return tickOnly();
  if (state !== was || event.type !== "tick") draw();
}

function tickOnly() {
  const left = sheet && sheet.querySelector(".am-wait .left");
  if (left && state.step === "waiting") left.textContent = `${timeLeft(state.expiresAt, Date.now())} left`;
}

function draw() {
  const box = sheet!;
  const head = el("div", "am-head", el("b", "", "Add a machine"), el("kbd", "", "esc"));
  const mine = macBlock();
  const other = mine && state.step !== "linked" ? el("div", "am-label am-other", "Another machine") : null;
  box.replaceChildren(head, ...present([mine, other, ...body()]));
  // The target field first: a focused host button would install on Enter.
  const first = box.querySelector<HTMLElement>(".am-target:not(:disabled)") || box.querySelector("button");
  if (first && !box.contains(document.activeElement)) first.focus();
}

function body() {
  switch (state.step) {
    case "loading":
      return [el("div", "am-wait", el("span", "dspin"), "Making a setup token")];
    case "waiting":
      return present([
        sshBlock(),
        el("div", "am-label am-hand", "Or install it by hand"),
        el("div", "am-label", "1. Install beep-node on the machine"),
        codeLine(INSTALL),
        el("div", "am-label", "2. Run this there"),
        codeLine(state.command),
        statusLine(),
      ]);
    case "linked": {
      const what = describeMachine(state.machine);
      return [
        el("div", "am-done", el("span", "ddot"), el("b", "", `${state.machine.name} linked`)),
        el("div", "am-sub", `${what ? what + ". " : ""}It shows under Machines and can run agents now.`),
        button("Done", "dbtn pri", closeAddMachine),
      ];
    }
    case "expired":
      return [
        el("div", "am-msg", "This token expired. Tokens last 10 minutes and work once."),
        button("New token", "dbtn pri", start),
      ];
    case "outdated":
      return [
        el("div", "am-msg", "The server needs an update before machines can be added from here."),
        button("Close", "dbtn", closeAddMachine),
      ];
    default:
      return [
        el("div", "am-msg bad", state.message || "Could not make a setup token."),
        el("div", "am-line", button("Try again", "dbtn pri", start), button("Close", "dbtn quiet", closeAddMachine)),
      ];
  }
}
