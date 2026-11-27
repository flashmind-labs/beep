// The Device tab's logic: the devices the machines list, which one shows,
// where the screen sits in its view, and how pointer and keys become the
// node's input (`device/1`). device.ts keeps the state; device-view.tsx draws it.

import { parseTool } from "../agents/agentlog.ts";

export type DevicePlatform = "Android" | "Ios";
type DevicePower = "Off" | "Booting" | "On";

/// An emulator, simulator or phone, as `device_list` gives it.
export interface Device {
  id: string;
  name: string;
  platform: DevicePlatform;
  state: DevicePower;
  os: string | null;
  /// A phone plugged in: always on, with no Boot or Shut down.
  phone: boolean;
}

/// One machine's devices, or why it could not list them.
export interface DeviceMachine {
  machine_id: string;
  machine_name: string;
  devices: Device[];
  error: string | null;
  /// The list has not arrived: the machine's devices are unknown, not none.
  loading: boolean;
}

/// A machine that serves `device/1`.
export interface DeviceHost {
  machine_id: string;
  machine_name: string;
}

/// The device the tab shows.
export interface DevicePick {
  machine: string;
  device: string;
}

export type TouchPhase = "Down" | "Move" | "Up";
type DeviceKey = "Return" | "Delete" | "Tab" | "Escape" | "Left" | "Right" | "Up" | "Down";
export type DeviceButton = "Home" | "Back" | "AppSwitch" | "Power" | "VolumeUp" | "VolumeDown" | "Rotate";

/// Input as the node's `Input` reads in JSON.
export type DeviceInput =
  | { Touch: { phase: TouchPhase; x: number; y: number } }
  | { Text: { text: string } }
  | { Key: { key: DeviceKey } }
  | { Button: { button: DeviceButton } };

export type VideoRotation = 0 | 1 | 2 | 3;
export type DeviceMedia = { kind: "jpeg" } | { kind: "h264"; width: number; height: number; rotation: VideoRotation };

/// What the screen shows.
export type ScreenState =
  | { kind: "off" }
  | { kind: "attaching" }
  | { kind: "live" }
  | { kind: "ended"; reason: string };

/// A message on `device_attach`'s channel.
type ScreenMessage =
  | { kind: "attached" }
  | { kind: "video"; fps: number }
  | { kind: "video_data"; data: Uint8Array<ArrayBuffer>; rotation: VideoRotation }
  | { kind: "frame"; width: number; height: number; jpeg: Uint8Array<ArrayBuffer> }
  | { kind: "ended"; reason: string };

export interface Size { width: number; height: number }
export interface Rect { x: number; y: number; width: number; height: number }

/// The smallest and largest frame side the node is asked for (`MAX_SIDE`).
const MIN_SIDE = 160;
const MAX_SIDE = 2048;
/// Frame sides asked for go up in these steps, so small resizes ask nothing new.
const SIDE_STEP = 128;

// ----- Devices -----

/// The device `pick` names among `machines`, or null.
export function pickedDevice(machines: readonly DeviceMachine[], pick: DevicePick | null): Device | null {
  if (!pick) return null;
  return machines.find(m => m.machine_id === pick.machine)?.devices.find(d => d.id === pick.device) ?? null;
}

/// The device to show: `last` while it is still listed, else one of the
/// project's `platforms` on the session's machine, else on any machine; a
/// running one first. Null when no machine has one.
export function defaultPick(machines: readonly DeviceMachine[], session: string | null, last: DevicePick | null, platforms: readonly DevicePlatform[]): DevicePick | null {
  if (last && pickedDevice(machines, last)) return last;
  const fits = (m: DeviceMachine) => m.devices.filter(d => platforms.includes(d.platform));
  const own = machines.find(m => m.machine_id === session && fits(m).length > 0) ?? null;
  const pool = own ? [own] : machines;
  const found = pool.flatMap(m => fits(m).map(device => ({ machine: m.machine_id, device })));
  const pick = found.find(f => f.device.state === "On") ?? found[0];
  return pick ? { machine: pick.machine, device: pick.device.id } : null;
}

/// The device a session keeps among `machines`: shown while it is on or
/// booting, forgotten once its machine lists it off or no longer has it. A
/// machine that is not listed, is still listing, or did not list its devices,
/// leaves it be.
export function keptPick(machines: readonly DeviceMachine[], kept: DevicePick | null): { pick: DevicePick | null; forget: boolean } {
  const machine = kept ? machines.find(m => m.machine_id === kept.machine) : null;
  if (!kept || !machine || machine.error !== null || machine.loading) return { pick: null, forget: false };
  const state = machine.devices.find(d => d.id === kept.device)?.state ?? "Off";
  return state === "Off" ? { pick: null, forget: true } : { pick: kept, forget: false };
}

/// The device a session keeps on its machine, or that its machine's
/// beep-node is too old to keep one.
export type KeptDevice = { kind: "kept"; pick: DevicePick | null } | { kind: "unsupported" };

/// `agent_device`'s answer, checked; null when it does not read.
export function readKeptDevice(value: unknown): KeptDevice | null {
  if (!isRecord(value)) return null;
  if (value.kind === "unsupported") return { kind: "unsupported" };
  if (value.kind !== "kept") return null;
  const pick = value.pick;
  if (pick === null) return { kind: "kept", pick: null };
  return isRecord(pick) && typeof pick.machine === "string" && typeof pick.device === "string" ? { kind: "kept", pick: { machine: pick.machine, device: pick.device } } : null;
}

/// Whether a session has the Device tab: its checkout builds for a mobile
/// platform and a machine serves `device/1`.
export const showsDeviceTab = (platforms: readonly DevicePlatform[], hosts: readonly DeviceHost[]) => platforms.length > 0 && hosts.length > 0;

const MISSING: Record<DevicePlatform, string> = {
  Android: "No machine has an Android emulator. Android Studio can make one.",
  Ios: "No machine has an iOS simulator. A Mac with Xcode and idb_companion can run one.",
};

/// The line for the project's platforms no machine has a device for, or
/// null when each has one.
export function missingLine(machines: readonly DeviceMachine[], platforms: readonly DevicePlatform[]): string | null {
  const missing = platforms.filter(p => !machines.some(m => m.devices.some(d => d.platform === p)));
  return missing.length > 0 ? missing.map(p => MISSING[p]).join(" ") : null;
}

/// `device_platforms`' answer, checked.
export function readPlatforms(value: unknown): DevicePlatform[] {
  if (!Array.isArray(value)) return [];
  return value.filter((p): p is DevicePlatform => p === "Android" || p === "Ios");
}

/// The picker's value for a pick, and back.
export const pickValue = (pick: DevicePick) => `${pick.machine}\n${pick.device}`;

export function readPickValue(value: string): DevicePick | null {
  const at = value.indexOf("\n");
  return at > 0 ? { machine: value.slice(0, at), device: value.slice(at + 1) } : null;
}

// ----- The agent's device calls -----

/// What the agent's `device` tool did, as the node words its title
/// (`Beep: Boot iPhone 17 on mac-box`).
const DEVICE_VERBS = ["Boot", "Shut down", "Show", "Capture", "Tap", "Swipe", "Type into", "Press"] as const;
export type DeviceVerb = typeof DEVICE_VERBS[number];

/// A `device` call on one device: the machine by its name in the agent's
/// list, the device by the name or id the agent gave, and what the title
/// adds (the point, the text or the key).
interface DeviceCall {
  verb: DeviceVerb;
  machine: string;
  device: string;
  detail: string;
}

/// The device a call names, which the Device tab turns to.
export type DeviceTarget = Pick<DeviceCall, "machine" | "device">;

const DEVICE_CALL = /^(Boot|Shut down|Show|Capture|Tap|Swipe|Type into) (.+?) on ([^\s:]+)(?:: ([\s\S]*))?$/;
const PRESS_CALL = /^Press (\S+) on (.+?) on ([^\s:]+)$/;

/// The `device` call a tool title names, or null for any other.
export function deviceCall(title: string): DeviceCall | null {
  const t = title.trim();
  if (!t.startsWith("Beep: ")) return null;
  const rest = t.slice("Beep: ".length);
  const press = PRESS_CALL.exec(rest);
  if (press) return { verb: "Press", device: press[2], machine: press[3], detail: press[1] };
  const m = DEVICE_CALL.exec(rest);
  const verb = DEVICE_VERBS.find(v => v === m?.[1]);
  if (!m || !verb) return null;
  return { verb, device: m[2], machine: m[3], detail: m[4] ?? "" };
}

/// What is known of a session's log for following its device calls: where
/// it started when the view opened (older pages come before it) and the
/// calls already seen to finish.
export interface DeviceFollow {
  from: number;
  seen: ReadonlySet<number>;
}

/// The device the agent's calls point the tab at: the last one that
/// finished since `follow`, other than a shutdown. The first look at a
/// session (`follow` null) with a log only notes what already finished, so
/// history does not move the tab.
export function followedDevice(entries: readonly { kind: string; text: string }[], earlier: number, follow: DeviceFollow | null): { follow: DeviceFollow | null; target: DeviceTarget | null } {
  if (!follow && entries.length === 0) return { follow: null, target: null };
  const done = entries.flatMap((entry, i) => {
    if (entry.kind !== "tool" || parseTool(entry.text).status !== "completed") return [];
    return [{ at: earlier + i, call: deviceCall(parseTool(entry.text).title) }];
  });
  const seen = new Set([...(follow?.seen ?? []), ...done.map(d => d.at)]);
  if (!follow) return { follow: { from: earlier, seen }, target: null };

  const fresh = done.filter(d => d.at >= follow.from && !follow.seen.has(d.at) && d.call && d.call.verb !== "Shut down");
  const last = fresh.at(-1)?.call ?? null;
  return { follow: { from: follow.from, seen }, target: last ? { machine: last.machine, device: last.device } : null };
}

/// A machine's name as the agent's tools give it: lower case, one word,
/// apostrophes dropped, other symbols and spaces as single dashes.
export function commandName(name: string) {
  return name.replace(/['\u2019]/g, "").toLowerCase().split(/[^\p{L}\p{N}]+/u).filter(Boolean).join("-");
}

/// The pick for `target` among `machines`: the device by id, else by a
/// name only one device on that machine has. Null when none fits.
export function targetPick(machines: readonly DeviceMachine[], target: DeviceTarget): DevicePick | null {
  const machine = machines.find(m => m.machine_id === target.machine || commandName(m.machine_name) === target.machine);
  if (!machine) return null;
  const named = target.device.trim();
  const byName = machine.devices.filter(d => d.name.toLowerCase() === named.toLowerCase());
  const device = machine.devices.find(d => d.id === named) ?? (byName.length === 1 ? byName[0] : null);
  return device ? { machine: machine.machine_id, device: device.id } : null;
}

/// The hardware buttons a device has, in the row's order.
export function buttonsFor(device: Device): DeviceButton[] {
  if (!takesInput(device)) return [];
  switch (device.platform) {
    case "Android": return ["Home", "Back", "AppSwitch", "Rotate", "Power", "VolumeDown", "VolumeUp"];
    case "Ios": return ["Home", "Rotate", "Power", "VolumeDown", "VolumeUp"];
  }
}

/// Whether `device` takes taps, keys and buttons: an iPhone only shows
/// its screen, as `devicectl` has no input.
export function takesInput(device: Device): boolean {
  return !(device.phone && device.platform === "Ios");
}

export const BUTTON_NAMES: Record<DeviceButton, string> = {
  Home: "Home", Back: "Back", AppSwitch: "Recent apps", Power: "Power", VolumeUp: "Volume up", VolumeDown: "Volume down", Rotate: "Rotate",
};

/// The chip of a device's state.
export function powerChip(state: DevicePower): { text: string; cls: string } {
  switch (state) {
    case "Off": return { text: "off", cls: "" };
    case "Booting": return { text: "booting", cls: "wait" };
    case "On": return { text: "on", cls: "run" };
  }
}

/// `device_list`'s answer, checked; machines or devices that do not read are left out.
export function readMachines(value: unknown): DeviceMachine[] {
  if (!Array.isArray(value)) return [];
  return value.flatMap(item => {
    if (!isRecord(item) || typeof item.machine_id !== "string" || typeof item.machine_name !== "string") return [];
    const devices = Array.isArray(item.devices) ? item.devices.flatMap(readDevice) : [];
    return [{ machine_id: item.machine_id, machine_name: item.machine_name, devices, error: typeof item.error === "string" ? item.error : null, loading: item.loading === true }];
  });
}

function readDevice(value: unknown): Device[] {
  if (!isRecord(value) || typeof value.id !== "string" || typeof value.name !== "string") return [];
  const platform = value.platform === "Android" || value.platform === "Ios" ? value.platform : null;
  const state = value.state === "Off" || value.state === "Booting" || value.state === "On" ? value.state : null;
  if (!platform || !state) return [];
  return [{ id: value.id, name: value.name, platform, state, os: typeof value.os === "string" ? value.os : null, phone: value.phone === true }];
}

/// What a node's `devices` topic pushed: its devices as one machine's list, or null for an update
/// that holds none. A node that predates the topic answers an error instead, which is `"refused"`.
export function readAnnounced(data: unknown): Device[] | "refused" | null {
  if (!isRecord(data)) return null;
  if (typeof data.error === "string") return "refused";
  return Array.isArray(data.Devices) ? data.Devices.flatMap(readDevice) : null;
}

/// `device_hosts`' answer, checked.
export function readHosts(value: unknown): DeviceHost[] {
  if (!Array.isArray(value)) return [];
  return value.flatMap(item => isRecord(item) && typeof item.machine_id === "string" && typeof item.machine_name === "string"
    ? [{ machine_id: item.machine_id, machine_name: item.machine_name }] : []);
}

const isRecord = (value: unknown): value is Record<string, unknown> => typeof value === "object" && value !== null && !Array.isArray(value);

// ----- The screen -----

/// Where an image of `image` size sits in a box of `box` size: as large as
/// fits, its aspect kept, centered.
export function fitRect(box: Size, image: Size): Rect {
  if (box.width <= 0 || box.height <= 0 || image.width <= 0 || image.height <= 0) return { x: 0, y: 0, width: 0, height: 0 };
  const scale = Math.min(box.width / image.width, box.height / image.height);
  const width = image.width * scale, height = image.height * scale;
  return { x: (box.width - width) / 2, y: (box.height - height) / 2, width, height };
}

export function rotatedSize(size: Size, rotation: VideoRotation): Size {
  return rotation === 1 || rotation === 3 ? { width: size.height, height: size.width } : size;
}

/// A point in the box as fractions of the drawn image, or null outside it.
/// With `clamp`, a point outside is moved to the image's edge, as a drag
/// that leaves the screen keeps touching its edge.
export function touchPoint(rect: Rect, x: number, y: number, clamp: boolean): { x: number; y: number } | null {
  if (rect.width <= 0 || rect.height <= 0) return null;
  const fx = (x - rect.x) / rect.width, fy = (y - rect.y) / rect.height;
  const inside = fx >= 0 && fx <= 1 && fy >= 0 && fy <= 1;
  if (!inside && !clamp) return null;
  return { x: Math.min(1, Math.max(0, fx)), y: Math.min(1, Math.max(0, fy)) };
}

/// The longest frame side worth asking for a view of `box` CSS pixels.
export function maxSide(box: Size, pixelRatio: number): number {
  const want = Math.max(box.width, box.height) * (pixelRatio > 0 ? pixelRatio : 1);
  const stepped = Math.ceil(want / SIDE_STEP) * SIDE_STEP;
  return Math.min(MAX_SIDE, Math.max(MIN_SIDE, stepped));
}

/// A channel message, or null for one this page does not know. A frame
/// comes raw: width and height as four little-endian bytes each, then the
/// JPEG. Tauri hands raw bytes over as an ArrayBuffer, or as an array of
/// numbers when its IPC falls back to postMessage.
export function readScreenMessage(message: unknown): ScreenMessage | null {
  const bytes = rawBytes(message);
  if (bytes) {
    if (bytes.length < 8) return null;
    const view = new DataView(bytes.buffer, bytes.byteOffset, 8);
    if (view.getUint32(0, true) === 0x31444956) {
      const rotation = view.getUint32(4, true);
      if (rotation !== 0 && rotation !== 1 && rotation !== 2 && rotation !== 3) return null;
      if (bytes.length <= 8 || bytes.length > 65544) return null;
      return { kind: "video_data", data: bytes.slice(8), rotation };
    }
    return { kind: "frame", width: view.getUint32(0, true), height: view.getUint32(4, true), jpeg: bytes.slice(8) };
  }

  if (!isRecord(message)) return null;
  if (message.kind === "video" && typeof message.fps === "number" && Number.isInteger(message.fps) && message.fps > 0 && message.fps <= 240) return { kind: "video", fps: message.fps };
  if (message.kind === "attached") return { kind: "attached" };
  if (message.kind === "ended" && typeof message.reason === "string") return { kind: "ended", reason: message.reason };
  return null;
}

function rawBytes(message: unknown): Uint8Array | null {
  if (message instanceof ArrayBuffer) return new Uint8Array(message);
  if (ArrayBuffer.isView(message)) return new Uint8Array(message.buffer, message.byteOffset, message.byteLength);
  if (Array.isArray(message) && message.every(n => typeof n === "number")) return Uint8Array.from(message);
  return null;
}

/// Input waiting while the input before it is on its way: a move replaces a
/// move waiting last, so a drag never falls behind the finger.
export function queueInput(queue: readonly DeviceInput[], input: DeviceInput): DeviceInput[] {
  const last = queue.at(-1);
  if (last && isMove(last) && isMove(input)) return [...queue.slice(0, -1), input];
  return [...queue, input];
}

const isMove = (input: DeviceInput) => "Touch" in input && input.Touch.phase === "Move";

// ----- Keys -----

const KEYS: Record<string, DeviceKey> = {
  Enter: "Return", Backspace: "Delete", Tab: "Tab", Escape: "Escape",
  ArrowLeft: "Left", ArrowRight: "Right", ArrowUp: "Up", ArrowDown: "Down",
};

/// What a key pressed on the focused screen sends: a key with no text of
/// its own, or null for text (which comes as typed) and for shortcuts with
/// ⌘, ⌃ or ⌥, which stay the app's.
export function keyInput(event: { key: string; metaKey: boolean; ctrlKey: boolean; altKey: boolean; isComposing: boolean }): DeviceInput | null {
  if (event.metaKey || event.ctrlKey || event.altKey || event.isComposing) return null;
  const key = KEYS[event.key];
  return key ? { Key: { key } } : null;
}

/// An error from a device command as the tab shows it.
export function deviceError(e: unknown): string {
  const said = e instanceof Error ? e.message : isRecord(e) && typeof e.message === "string" ? e.message : String(e);
  const text = said.replace(/^bad request: /, "");
  if (/unknown protocol|not serve|refused/i.test(text)) return "beep-node there is too old for devices. Update it.";
  return text;
}
