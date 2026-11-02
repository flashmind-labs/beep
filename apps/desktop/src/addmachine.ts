// The "Add a machine" sheet. First, this Mac in one click: "Use this Mac"
// turns on Let Beep use this Mac. Below it, for another machine, Beep makes
// a one-time setup token, the machine
// claims it with `beep-node setup <token>` and links itself to this
// account. The sheet waits, asking which machine claimed the token, and flips to Linked
// by itself. Esc or Cancel closes it and revokes a token nobody used.
import { INSTALL, START, next, timeLeft, machineById, describeMachine, macOption } from "./addmachine-state.ts";
import { present } from "./projects-state.ts";

const { invoke } = window.__TAURI__.core;

const POLL_MS = 2000;
const TICK_MS = 1000;

let root = null, sheet = null, state = START;
let ticker = null, poller = null, back = null, onClose = null;
// This Mac's mac_control view, and whether the sheet turned it on.
let mac = null, macAsked = false, macPoller = null;

function el(tag, cls, ...kids) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  for (const k of kids) if (k != null && k !== false) e.append(k);
  return e;
}

function button(label, cls, onClick) {
  const b = el("button", cls, label);
  b.type = "button";
  b.addEventListener("click", onClick);
  return b;
}

function copyButton(text) {
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

function codeLine(text) {
  return el("div", "am-line", el("div", "am-code", text), copyButton(text));
}

/// Opens the sheet over the window. `options.onClose` runs once it closes,
/// as when the window wants to refresh its machines.
export function openAddMachine(options = {}) {
  if (root) return;
  onClose = options.onClose || null;
  back = document.activeElement;
  injectStyle();
  root = el("div", "am-scrim");
  sheet = el("div", "am-sheet");
  sheet.setAttribute("role", "dialog");
  sheet.setAttribute("aria-modal", "true");
  sheet.setAttribute("aria-label", "Add a machine");
  root.append(sheet);
  document.body.append(root);
  document.addEventListener("keydown", onKey, true);
  ticker = setInterval(() => update({ type: "tick", now: Date.now() }), TICK_MS);
  mac = null;
  macAsked = false;
  loadMac();
  macPoller = setInterval(loadMac, POLL_MS);
  start();
}

/// Closes the sheet, revoking its token unless the machine used it.
export function closeAddMachine() {
  if (!root) return;
  if (state.step === "waiting") invoke("revoke_node_token", { token: state.token }).catch(() => {});
  clearInterval(ticker);
  clearInterval(poller);
  clearInterval(macPoller);
  ticker = poller = macPoller = null;
  document.removeEventListener("keydown", onKey, true);
  root.remove();
  root = sheet = null;
  state = START;
  if (back && back.focus) back.focus();
  const done = onClose;
  onClose = null;
  if (done) done();
}

function onKey(e) {
  if (e.key !== "Escape") return;
  e.preventDefault();
  e.stopPropagation();
  closeAddMachine();
}

/// Asks for a token.
async function start() {
  update({ type: "renew" });
  clearInterval(poller);
  poller = null;
  try {
    const made = await invoke("create_node_token", { name: null });
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
    const id = await invoke("node_token_machine", { token });
    if (!id) return;
    // Matched by id, so a machine set up again is found too.
    const machine = machineById(await invoke("list_machines"), id);
    if (machine && state.token === token) update({ type: "found", machine });
  } catch {
    // The next poll tries again.
  }
}

/// Reads where this Mac stands and redraws its option.
async function loadMac() {
  try {
    const view = await invoke("mac_control");
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
    mac = await invoke("set_mac_control", { enabled: true });
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

function update(event) {
  const was = state;
  state = next(state, event);
  if (state.step !== "waiting") {
    clearInterval(poller);
    poller = null;
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
  const head = el("div", "am-head", el("b", "", "Add a machine"), el("kbd", "", "esc"));
  const mine = macBlock();
  const other = mine && state.step !== "linked" ? el("div", "am-label am-other", "Another machine") : null;
  sheet.replaceChildren(head, ...present([mine, other, ...body()]));
  const first = sheet.querySelector("button");
  if (first && !sheet.contains(document.activeElement)) first.focus();
}

function body() {
  switch (state.step) {
    case "loading":
      return [el("div", "am-wait", el("span", "dspin"), "Making a setup token")];
    case "waiting":
      return [
        el("div", "am-label", "1. Install beep-node on the machine"),
        codeLine(INSTALL),
        el("div", "am-label", "2. Run this there"),
        codeLine(state.command),
        el("div", "am-wait",
          el("span", "dspin"),
          "Waiting for the machine · token works once, ",
          el("span", "left", `${timeLeft(state.expiresAt, Date.now())} left`),
          el("span", "end", button("Cancel", "dbtn quiet", closeAddMachine))),
      ];
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

/// Loads the sheet's styles once, so the window only has to import this.
function injectStyle() {
  if (document.querySelector('link[data-am]')) return;
  const link = document.createElement("link");
  link.rel = "stylesheet";
  link.href = new URL("./addmachine.css", import.meta.url).href;
  link.dataset.am = "";
  document.head.append(link);
}
