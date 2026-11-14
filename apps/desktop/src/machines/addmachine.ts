// The "Add a machine" sheet.
import { subscribeChanges, topic } from "../ui/mesh-events.ts";
import { latest, onShownChange, STALE } from "../ui/active.ts";
import { invoke } from "../ui/tauri.ts";
import { START, next, machineById, inviteArgs } from "./addmachine-state.ts";
import type { AddEvent, MacSeen, TicketKind } from "./addmachine-state.ts";
import { createElement, type ReactNode } from "react";
import { flushSync } from "react-dom";
import { openSheet } from "../ui/parts.ts";
import { AddMachineBody } from "./addmachine-view.tsx";
import type { MacControl, MachineView } from "./machine-state.ts";

/// A ticket as `mesh_invite` gives it (TokenView).
interface NodeToken {
  token: string;
  command: string;
  qr_svg: string | null;
  /// In milliseconds since the Unix epoch.
  expires_at: number;
}

type Timer = ReturnType<typeof setInterval> | undefined;

const TICK_MS = 1000;

let root: HTMLDivElement | null = null, sheet: HTMLDivElement | null = null, state = START;
/// What the ticket is for: a node, a container, another Mac, or a phone.
let kind: TicketKind = "node";
const creation = latest();
let ticker: Timer, poller: (() => void) | undefined, close: (() => void) | null = null, onClose: (() => void) | null = null;
// This Mac's mac_control view, and whether the sheet turned it on.
let mac: MacSeen | null = null, macAsked = false, macPoller: (() => void) | undefined;
let render: ((content: ReactNode) => void) | null = null;
let opening = 0;

onShownChange(visible => {
  clearInterval(ticker);
  ticker = undefined;

  if (visible && root) {
    update({ type: "tick", now: Date.now() });
    ticker = setInterval(() => update({ type: "tick", now: Date.now() }), TICK_MS);
  }
});

/// Opens the sheet over the window. `options.onClose` runs once it closes,
/// as when the window wants to refresh its machines.
export function openAddMachine(options: { onClose?: () => void } = {}) {
  if (root) return;
  onClose = options.onClose || null;
  opening++;
  kind = "node";
  ({ root, sheet, close, render } = openSheet("Add a machine", "am-sheet", { onClose: closed }));
  ticker = setInterval(() => update({ type: "tick", now: Date.now() }), TICK_MS);
  mac = null;
  macAsked = false;
  loadMac();
  macPoller = subscribeChanges(topic("machines"), { deliver: "latest", receive: () => loadMac() });
  start();
}

/// Closes the sheet.
export function closeAddMachine() {
  close?.();
}

function closed() {
  creation.cancel();
  opening++;
  render = null;
  clearInterval(ticker);
  poller?.();
  macPoller?.();
  ticker = undefined;
  poller = macPoller = undefined;
  root = sheet = close = null;
  state = START;
  const done = onClose;
  onClose = null;
  if (done) done();
}

/// Asks for a token.
async function start() {
  update({ type: "renew" });
  poller?.();
  poller = undefined;

  try {
    const made = await creation.run(invoke<NodeToken>("mesh_invite", inviteArgs(kind)));
    if (!root || made === STALE) return;
    update({ type: "created", token: made.token, command: made.command, expiresAt: made.expires_at, qrSvg: made.qr_svg ?? undefined });
    poller = subscribeChanges([topic("machines"), topic("projects")], { deliver: "latest", receive: () => poll() });
    void poll();
  } catch (e) {
    if (!root) return;
    update({ type: "failed", message: String(e) });
  }
}

async function poll() {
  if (state.step !== "waiting") return;
  const token = state.token;

  try {
    const id = await invoke<string | null>("ticket_machine", { ticket: token });
    if (!id) return;

    if (kind === "phone") {
      if (state.step === "waiting" && state.token === token) update({ type: "paired" });
      return;
    }

    // Matched by id, so a machine set up again is found too.
    const machine = machineById(await invoke<MachineView[]>("list_machines"), id);
    if (machine && state.step === "waiting" && state.token === token) update({ type: "found", machine });
  } catch {
    // A membership update or reconnect tries again.
  }
}

/// Reads where this Mac stands and redraws its option.
async function loadMac() {
  const asked = opening;
  try {
    const view = await invoke<MacControl>("mac_control");
    if (!root || asked !== opening) return;
    mac = view;
  } catch (e) {
    console.warn("this computer's state:", e);
    return;
  }

  draw(false);
}

/// Turns on Let Beep use this Mac.
async function useThisMac() {
  const asked = opening;
  macAsked = true;

  try {
    const view = await invoke<MacControl>("set_mac_control", { enabled: true });
    if (!root || asked !== opening) return;
    mac = view;
  } catch (e) {
    if (!root || asked !== opening) return;
    mac = { ...mac, enabled: true, status: "failed", error: String(e) };
  }

  if (root) draw(false);
}

function update(event: AddEvent) {
  const previous = state;
  state = next(state, event);
  if (state.step !== "waiting") { poller?.(); poller = undefined; }
  if (root) draw(event.type !== "tick" || state !== previous);
}

function draw(focus = true) {
  if (!sheet || !render) return;
  const box = sheet, show = render;
  flushSync(() => show(createElement(AddMachineBody, {
    state, kind, mac, macAsked, now: Date.now(),
    selectKind: (selected: TicketKind) => { if (kind !== selected) { kind = selected; void start(); } },
    useThisMac: () => { void useThisMac(); }, start: () => { void start(); }, close: closeAddMachine,
  })));
  const first = box.querySelector("button");
  if (focus && first && !box.contains(document.activeElement)) first.focus();
}
