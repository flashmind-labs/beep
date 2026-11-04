// The "Add a machine" sheet.
import { invoke } from "../ui/tauri.ts";
import { HOME_SEARCH, INSTALL, INSTALL_WINDOWS, START, next, timeLeft, machineById, describeMachine, macOption, withSearch } from "./addmachine-state.ts";
import type { AddEvent, MacSeen } from "./addmachine-state.ts";
import type { MacControl, MachineView } from "../center/home-state.ts";
import { button, el } from "../ui/dom.ts";
import { copyButton, focusBack, sheetShell } from "../ui/parts.ts";
import { present } from "../addproject/importproject-state.ts";
import { openJoinTicket } from "./jointicket.ts";

/// A ticket as `mesh_invite` gives it (TokenView).
interface NodeToken {
  token: string;
  command: string;
  /// In milliseconds since the Unix epoch.
  expires_at: number;
}

type Timer = ReturnType<typeof setInterval> | undefined;

const POLL_MS = 2000;
const TICK_MS = 1000;

let root: HTMLDivElement | null = null, sheet: HTMLDivElement | null = null, state = START;
/// What the ticket is for: a node, or another Mac.
let kind: "node" | "desktop" = "node";
let ticker: Timer, poller: Timer, back: Element | null = null, onClose: (() => void) | null = null;
/// `onClose` of the sheet that opened, for the join sheet it hands over to.
let onCloseAgain: (() => void) | null = null;
// This Mac's mac_control view, and whether the sheet turned it on.
let mac: MacSeen | null = null, macAsked = false, macPoller: Timer;
// Where the new machine looks for projects; it goes on the setup command.
let searchInput: HTMLInputElement | null = null;

function codeLine(text: string) {
  return el("div", "am-line", el("div", "am-code", text), copyButton(text));
}

/// Opens the sheet over the window. `options.onClose` runs once it closes,
/// as when the window wants to refresh its machines.
export function openAddMachine(options: { onClose?: () => void } = {}) {
  if (root) return;
  onClose = options.onClose || null;
  onCloseAgain = onClose;
  kind = "node";
  back = document.activeElement;
  ({ root, sheet } = sheetShell("Add a machine", "am-sheet"));
  document.body.append(root);
  document.addEventListener("keydown", onKey, true);
  ticker = setInterval(() => update({ type: "tick", now: Date.now() }), TICK_MS);
  mac = null;
  macAsked = false;
  loadMac();
  macPoller = setInterval(loadMac, POLL_MS);
  searchInput = el("input", "am-code am-search");
  searchInput.value = HOME_SEARCH;
  searchInput.setAttribute("aria-label", "Search for projects in");
  searchInput.spellcheck = false;
  searchInput.addEventListener("input", () => {
    const shown = sheet && sheet.querySelector(".am-cmd");
    const now = commandLine();
    if (shown && now) shown.replaceWith(now);
  });
  start();
}

/// The setup command to copy, with the search folder when it is not home.
function commandLine() {
  if (state.step !== "waiting") return null;
  const line = codeLine(withSearch(state.command, searchInput ? searchInput.value : ""));
  line.classList.add("am-cmd");
  return line;
}

/// Closes the sheet.
export function closeAddMachine() {
  if (!root) return;
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
    const made = await invoke<NodeToken>("mesh_invite", { kind: kind === "node" ? "Node" : "Desktop" });
    if (!root) return;
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

/// Where the sheet says how it goes: the wait for the token.
function statusLine() {
  if (state.step !== "waiting") return el("div", "am-status");
  return el("div", "am-status am-wait",
    el("span", "dspin"),
    `Waiting for ${kind === "node" ? "the machine" : "the other Mac"} · works once, `,
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
  draw();
}

function tickOnly() {
  const left = sheet && sheet.querySelector(".am-wait .left");
  if (left && state.step === "waiting") left.textContent = `${timeLeft(state.expiresAt, Date.now())} left`;
}

function draw() {
  const box = sheet!;
  const mine = macBlock();
  const other = mine && state.step !== "linked" ? el("div", "am-label am-other", "Another machine") : null;
  const done = state.step === "linked" || state.step === "expired";
  box.replaceChildren(el("div", "am-head", el("b", "", "Add a machine"), el("kbd", "", "esc")), ...present([done ? null : kindPicker(), mine, other, ...body(), done ? null : button("Join with a ticket", "dbtn quiet", () => { closeAddMachine(); openJoinTicket({ onClose: onCloseAgain ?? undefined }); })]));
  const first = box.querySelector("button");
  if (first && !box.contains(document.activeElement)) first.focus();
}

/// Node or Desktop: what the ticket is for.
function kindPicker() {
  const card = (k: "node" | "desktop", title: string, sub: string) => {
    const b = el("button", "dbtn am-kind", el("b", "", title), el("span", "", sub));
    b.type = "button";
    b.setAttribute("role", "radio");
    b.setAttribute("aria-checked", String(kind === k));
    b.addEventListener("click", () => { if (kind === k) return; kind = k; start(); });
    return b;
  };
  const group = el("div", "am-kinds", card("node", "Node", "A server or any always-on box"), card("desktop", "Desktop", "Another Mac"));
  group.setAttribute("role", "radiogroup");
  group.setAttribute("aria-label", "Kind of machine");
  return group;
}

function body() {
  switch (state.step) {
    case "loading":
      return [el("div", "am-wait", el("span", "dspin"), "Making a ticket")];
    case "waiting":
      if (kind === "desktop") {
        return present([
          el("div", "am-label", "Ticket"),
          codeLine(state.command),
          el("div", "am-os", "Paste it into Beep on the other Mac, under Join with a ticket."),
          statusLine(),
        ]);
      }
      return present([
        el("div", "am-line", el("span", "am-label am-search-label", "Search for projects in"), searchInput),
        el("div", "am-label", "1. Install beep-node on the machine"),
        el("div", "am-os", "macOS or Linux"),
        codeLine(INSTALL),
        el("div", "am-os", "Windows, in PowerShell"),
        codeLine(INSTALL_WINDOWS),
        el("div", "am-label", "2. Run this there"),
        commandLine(),
        statusLine(),
      ]);
    case "linked": {
      const what = describeMachine(state.machine);
      return [
        el("div", "am-done", el("span", "ddot"), el("b", "", `${state.machine.name} joined`)),
        el("div", "am-sub", `${what ? what + ". " : ""}It shows under Machines and can run agents now.`),
        button("Done", "dbtn pri", closeAddMachine),
      ];
    }
    case "expired":
      return [el("div", "am-msg", "This ticket expired. A ticket works once and lasts an hour."), button("New ticket", "dbtn pri", start)];
    default:
      return [
        el("div", "am-msg bad", state.message || "Could not make a ticket."),
        el("div", "am-line", button("Try again", "dbtn pri", start), button("Close", "dbtn quiet", closeAddMachine)),
      ];
  }
}
