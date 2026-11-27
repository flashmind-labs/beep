// The "Add a machine" sheet.
import { ensureMac, ensureMachines, MACHINES, machinesChanged, takeMac } from "../../stores/machines/machine-store.ts";
import { latest, onShownChange, STALE } from "../../ui/active.ts";
import { buildCapabilities, invoke } from "../../ui/tauri.ts";
import { subscribeEach, topic } from "../../ui/mesh-events.ts";
import { pushedView } from "../../ui/snapshot-state.ts";
import { START, next, machineById, inviteArgs } from "../../stores/machines/addmachine-state.ts";
import type { AddEvent, MacSeen, TicketKind } from "../../stores/machines/addmachine-state.ts";
import { type ReactNode } from "react";
import { flushSync } from "react-dom";
import { openSheet } from "../../ui/parts.tsx";
import { AddMachineBody } from "./addmachine-view.tsx";
import type { MacControl } from "../../parts/machines/machine-state.ts";

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
let mac: MacSeen | null = null, macAsked = false;
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
  machinesChanged.add(macChanged);
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
  ticker = undefined;
  poller = undefined;
  machinesChanged.delete(macChanged);
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
    // Read once; the machines update says when the ticket is taken.
    poller = subscribeEach([topic("machines"), topic("projects")], event => {
      const pushed = pushedView<string | null>(event.data, "ticket_machine", { ticket: made.token });
      if (pushed && "value" in pushed) void matched(pushed.value, made.token);
    });
    void poll();
  } catch (e) {
    if (!root) return;
    update({ type: "failed", message: String(e) });
  }
}

/// Asks once whether the ticket has been taken.
async function poll() {
  if (state.step !== "waiting") return;
  const token = state.token;

  try {
    await matched(await invoke<string | null>("ticket_machine", { ticket: token }), token);
  } catch {
    // A membership update or reconnect tells again.
  }
}

/// The machine `id` took the ticket `token`, when it is still the one the sheet waits on.
async function matched(id: string | null, token: string) {
  if (!id) return;

  if (kind === "phone") {
    if (state.step === "waiting" && state.token === token) update({ type: "paired" });
    return;
  }

  // Matched by id, so a machine set up again is found too.
  try {
    const machine = machineById(await ensureMachines(), id);
    if (machine && state.step === "waiting" && state.token === token) update({ type: "found", machine });
  } catch {
    // The next machines update tells again.
  }
}

/// Takes this Mac's state as the store holds it and redraws its option.
function macChanged() {
  if (!root || !MACHINES.mac) return;

  mac = MACHINES.mac;
  draw(false);
}

/// Reads where this Mac stands once, and redraws its option.
async function loadMac() {
  const asked = opening;
  try {
    if (!(await buildCapabilities()).local_host) return;
    const view = await ensureMac();
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
    takeMac(view);
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
  flushSync(() => show(<AddMachineBody
    state={state} kind={kind} mac={mac} macAsked={macAsked} now={Date.now()}
    selectKind={(selected: TicketKind) => { if (kind !== selected) { kind = selected; void start(); } }}
    useThisMac={() => { void useThisMac(); }} start={() => { void start(); }} close={closeAddMachine}
  />));
  const first = box.querySelector("button");
  if (focus && first && !box.contains(document.activeElement)) first.focus();
}
