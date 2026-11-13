// The Device tab: the machines that serve `device/1`, the device it shows,
// and that device's live screen while the tab and the window show. Frames go
// to the painter device-view.tsx registers; the rest is `deviceModel`.
import { createStore } from "zustand";
import { indexedChannel, invoke } from "../ui/tauri.ts";
import { subscribeChanges, topic } from "../ui/mesh-events.ts";
import { AMBIENT_MS, latest, onShownChange, paneShown, STALE } from "../ui/active.ts";
import { paneSession } from "./media.ts";
import { draw } from "./media-draw.ts";
import {
  type DeviceHost, type DeviceInput, type DeviceMachine, type DevicePick, type ScreenState,
  defaultPick, deviceError, pickedDevice, queueInput, readHosts, readMachines, readScreenMessage,
} from "./device-state.ts";

/// How often the list is read again while the tab shows: a booting device
/// soon, otherwise now and then, as devices start and stop outside Beep.
const BOOTING_POLL_MS = 2000;
const IDLE_POLL_MS = 10_000;

export interface DeviceModel {
  machines: readonly DeviceMachine[];
  /// The list was read at least once.
  listed: boolean;
  error: string;
  pick: DevicePick | null;
  /// A boot or shutdown is on its way.
  busy: boolean;
  screen: ScreenState;
}

export const deviceModel = createStore<DeviceModel>(() => ({ machines: [], listed: false, error: "", pick: null, busy: false, screen: { kind: "off" } }));

const set = (change: Partial<DeviceModel>) => deviceModel.setState(change);

// ----- The tab -----

let hosts: DeviceHost[] = [];

/// Whether the pane has a Device tab: a machine that is on serves `device/1`.
export const hasDeviceTab = () => hosts.length > 0;

async function readDeviceHosts() {
  const had = hasDeviceTab();

  try {
    hosts = readHosts(await invoke("device_hosts"));
  } catch {
    return;
  }

  if (had !== hasDeviceTab()) draw();
}

subscribeChanges(topic("machines"), { deliver: "latest", receive: readDeviceHosts }, { when: paneShown, wait: AMBIENT_MS });
if (typeof window !== "undefined") {
  window.addEventListener("agent-pane-shown", () => void readDeviceHosts());
  queueMicrotask(() => void readDeviceHosts());
}

// ----- What shows -----

/// The view is on screen in its dock.
let viewShown = false;

const visible = () => viewShown && paneShown();

/// The Device view showed or hid in its dock.
export function setDeviceShown(shown: boolean) {
  if (viewShown === shown) return;
  viewShown = shown;
  shownChanged();
}

onShownChange(() => shownChanged(), true);

function shownChanged() {
  if (!visible()) {
    clearTimeout(poll);
    detach();
    return;
  }

  // Shown again, a view that ended is tried again.
  if (deviceModel.getState().screen.kind === "ended") set({ screen: { kind: "off" } });
  void readDevices();
}

/// Shows device `pick`.
export function pickDevice(pick: DevicePick) {
  set({ pick, screen: { kind: "off" }, error: "" });
  follow();
}

/// The view of the device ended: it is tried again.
export function showAgain() {
  set({ screen: { kind: "off" } });
  void readDevices();
}

// ----- The list -----

const reads = latest();
let poll: ReturnType<typeof setTimeout> | undefined;

async function readDevices() {
  clearTimeout(poll);
  if (!visible()) return;

  try {
    const value = await reads.run(invoke<unknown>("device_list", { machineId: null }));
    if (value === STALE) return;
    const machines = readMachines(value);
    const pick = defaultPick(machines, paneSession()?.machine_id ?? null, deviceModel.getState().pick);
    set({ machines, pick, listed: true, error: "" });
  } catch (e) {
    set({ listed: true, error: deviceError(e) });
  }

  follow();
  if (!visible()) return;
  const state = pickedDevice(deviceModel.getState().machines, deviceModel.getState().pick)?.state;
  if (state === "On" && live) return;
  poll = setTimeout(() => void readDevices(), state === "Booting" ? BOOTING_POLL_MS : IDLE_POLL_MS);
}

/// Boots the device shown, or shuts it down.
export async function power(on: boolean) {
  const { machines, pick } = deviceModel.getState();
  const device = pickedDevice(machines, pick);
  if (!pick || !device) return;
  set({ busy: true, error: "" });
  if (!on) detach();

  try {
    await invoke(on ? "device_boot" : "device_shutdown", { machineId: pick.machine, id: device.id });
  } catch (e) {
    set({ error: deviceError(e) });
  } finally {
    set({ busy: false });
  }

  await readDevices();
}

// ----- The live screen -----

/// Draws a frame; it owns the bitmap from then on.
export type Painter = (frame: ImageBitmap) => void;

let painter: Painter | null = null;
/// The size of the box the screen draws in, for the frame size asked.
let sideFor: () => number = () => 0;

/// The view's canvas takes the frames, and says how large they should be.
export function setPainter(paint: Painter | null, side: () => number) {
  painter = paint;
  sideFor = side;
}

/// The attached device and the name of its view.
let live: { view: string; pick: DevicePick } | null = null;

const samePick = (a: DevicePick, b: DevicePick) => a.machine === b.machine && a.device === b.device;

/// Attaches to the device shown when it is on and the view shows; detaches
/// from one that no longer is.
function follow() {
  const { machines, pick, screen } = deviceModel.getState();
  const device = pickedDevice(machines, pick);
  const want = visible() && pick && device?.state === "On" ? pick : null;
  if (live && (!want || !samePick(live.pick, want))) detach();
  if (!want || live || screen.kind === "ended") return;
  attach(want, device?.id ?? want.device);
}

function attach(pick: DevicePick, id: string) {
  const view = crypto.randomUUID();
  const mine = { view, pick };
  live = mine;
  queue = [];
  set({ screen: { kind: "attaching" } });
  const channel = indexedChannel(message => heard(mine, message));
  invoke("device_attach", { machineId: pick.machine, id, view, maxSide: sideFor(), onEvent: channel })
    .then(() => { if (live === mine) ended(mine, "The view ended."); })
    .catch(e => { if (live === mine) ended(mine, deviceError(e)); });
}

function detach() {
  if (!live) return;
  const { view } = live;
  live = null;
  queue = [];
  set({ screen: { kind: "off" } });
  invoke("device_detach", { view }).catch(() => {});
}

function ended(mine: NonNullable<typeof live>, reason: string) {
  if (live !== mine) return;
  live = null;
  set({ screen: { kind: "ended", reason } });
  // It may have stopped: the list says.
  void readDevices();
}

function heard(mine: NonNullable<typeof live>, message: unknown) {
  if (live !== mine) return;
  const read = readScreenMessage(message);
  if (!read) return;

  switch (read.kind) {
    case "attached":
      set({ screen: { kind: "live" } });
      return;
    case "ended":
      ended(mine, read.reason);
      return;
    case "frame":
      void paint(mine, read.jpeg);
      return;
  }
}

/// Decodes a frame and hands it to the painter, then asks for the next.
async function paint(mine: NonNullable<typeof live>, jpeg: Uint8Array<ArrayBuffer>) {
  try {
    const bitmap = await createImageBitmap(new Blob([jpeg], { type: "image/jpeg" }));
    if (live === mine && painter) painter(bitmap);
    else bitmap.close();
  } catch {
    // A frame that does not decode is skipped.
  } finally {
    if (live === mine) invoke("device_drawn", { view: mine.view }).catch(() => {});
  }
}

// ----- Input -----

/// Input waiting while the input before it goes, in order.
let queue: DeviceInput[] = [];
let sending = false;

/// Sends `input` to the attached device; nothing while none is.
export function sendInput(input: DeviceInput) {
  if (!live || deviceModel.getState().screen.kind !== "live") return;
  queue = queueInput(queue, input);
  void drain();
}

async function drain() {
  if (sending) return;
  sending = true;

  try {
    for (let next = queue.shift(); next && live; next = queue.shift()) {
      await invoke("device_input", { view: live.view, input: next }).catch(() => {});
    }
  } finally {
    sending = false;
  }
}
