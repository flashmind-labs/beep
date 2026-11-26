import type JMuxer from "jmuxer";
// The Device tab: shown for a session whose checkout builds for Android or
// iOS, the machines that serve `device/1`, the device it shows, and that
// device's live screen while the tab and the window show. Frames go
// to the painter device-view.tsx registers; the rest is `deviceModel`.
import { createStore } from "zustand";
import { indexedChannel, invoke, listen } from "../ui/tauri.ts";
import { subscribeChanges, subscribeEach, topic } from "../ui/mesh-events.ts";
import { pushedList } from "../ui/snapshot-state.ts";
import { ambient, latest, onShownChange, paneShown, STALE } from "../ui/active.ts";
import { paneSession, redraw } from "./pane-state.ts";
import { dockOf, paneModel, setOn, shell } from "./media-layout.ts";
import { DEVICE } from "./media-state.ts";
import { rememberPane, saveLayout } from "./pane-memory.ts";
import { requestRightPane, setFolded } from "./pane-open.ts";
import { sessionKey } from "./running.ts";
import { paneRoot } from "./worktree.ts";
import {
  type DeviceMedia, type VideoRotation, type DeviceFollow, type DeviceHost, type KeptDevice, type DeviceInput, type Device, type DeviceMachine, type DevicePick, type DevicePlatform, type DeviceTarget, type ScreenState,
  defaultPick, deviceError, readAnnounced, followedDevice, keptPick, pickedDevice, queueInput, readHosts, readKeptDevice, readMachines, readPlatforms, readScreenMessage, showsDeviceTab, targetPick,
} from "./device-state.ts";

/// How often the list is read again while the tab shows, for the machines whose node does not
/// push its devices: a booting device soon, otherwise now and then. A node that pushes them is not
/// read: it follows adb, booting devices and iOS simulators itself and pushes each change.
/// How long a change waits for others before the list is read.
const DEVICES_WAIT_MS = 300;
const BOOTING_POLL_MS = 2000;
const IDLE_POLL_MS = 60_000;
/// How long a platform check that could not reach its machine waits to go again.
const PLATFORMS_RETRY_MS = 60_000;

interface DeviceModel {
  machines: readonly DeviceMachine[];
  /// The list was read at least once.
  listed: boolean;
  error: string;
  pick: DevicePick | null;
  /// The platforms the session's checkout builds for.
  platforms: readonly DevicePlatform[];
  /// A boot or shutdown is on its way.
  busy: boolean;
  screen: ScreenState;
  media: DeviceMedia;
}

export const deviceModel = createStore<DeviceModel>(() => ({
  machines: [], listed: false, error: "", pick: null, platforms: [], busy: false, screen: { kind: "off" }, media: { kind: "jpeg" },
}));

const set = (change: Partial<DeviceModel>) => deviceModel.setState(change);

// ----- The tab -----

let hosts: DeviceHost[] = [];
/// Whether a push gave the hosts, which an answer to the first read then does not replace.
let pushedHosts = false;

/// Whether the pane has a Device tab: a machine that is on serves
/// `device/1`, and the session's checkout builds for a mobile platform or
/// its agent used a device.
export const hasDeviceTab = () =>
  hosts.length > 0 && ((agentUsed !== null && agentUsed === sessionKey(paneSession())) || showsDeviceTab(sessionPlatforms(), hosts));

// The `machines` push carries the hosts; one read answers before the first push.
function takeDeviceHosts(list: unknown) {
  const had = hasDeviceTab();
  hosts = readHosts(list);
  if (had !== hasDeviceTab()) redraw();
  if (visible()) devicesChanged();
}

subscribeEach(topic("machines"), event => {
  const pushed = pushedList<unknown>(event.data, "device_hosts");
  if (pushed && "list" in pushed) { pushedHosts = true; takeDeviceHosts(pushed.list); }
});
if (typeof window !== "undefined") {
  queueMicrotask(() => void invoke("device_hosts").then(list => { if (!pushedHosts) takeDeviceHosts(list); }, () => {}));
}

// ----- The project's platforms -----

/// A session's checkout: its machine and folder.
interface Checkout { machine: string; root: string; key: string }

type PlatformState = { kind: "asking" } | { kind: "known"; platforms: DevicePlatform[] } | { kind: "retry"; after: number };

/// The platforms of each checkout asked about, by `Checkout.key`; asked
/// once, as a project rarely gains or loses one.
const platformsOf = new Map<string, PlatformState>();

function paneCheckout(): Checkout | null {
  const session = paneSession(), root = paneRoot();
  if (!session || !root) return null;
  return { machine: session.machine_id, root, key: `${session.machine_id}\n${root}` };
}

/// The platforms the pane's checkout builds for; the first time, none
/// while its machine is asked.
function sessionPlatforms(): DevicePlatform[] {
  const at = paneCheckout();
  if (!at) return [];
  const state = platformsOf.get(at.key);
  if (state?.kind === "known") return state.platforms;
  void askPlatforms(at, state);
  return [];
}

async function askPlatforms(at: Checkout, state = platformsOf.get(at.key)) {
  if (state?.kind === "asking" || (state?.kind === "retry" && Date.now() < state.after)) return;
  platformsOf.set(at.key, { kind: "asking" });

  try {
    const platforms = readPlatforms(await invoke("device_platforms", { machineId: at.machine, root: at.root }));
    platformsOf.set(at.key, { kind: "known", platforms });
    if (platforms.length > 0) redraw();
  } catch {
    platformsOf.set(at.key, { kind: "retry", after: Date.now() + PLATFORMS_RETRY_MS });
  }
}

// ----- The agent's device calls -----

/// The session whose agent used a device: it has the tab whatever its project.
let agentUsed: string | null = null;
/// The session whose log is followed, and what is known of it.
let followed: { session: string; follow: DeviceFollow | null } | null = null;
/// The device the agent last pointed at, until the list has it.
let wanted: DeviceTarget | null = null;

/// The conversation as drawn, from place `earlier`: a device call of the
/// agent that finished since the view opened turns the tab to its device.
export function followDeviceCalls(entries: readonly { kind: string; text: string }[], earlier: number) {
  const session = sessionKey(paneSession());
  if (!session) return;
  const { follow, target } = followedDevice(entries, earlier, followed?.session === session ? followed.follow : null);
  followed = { session, follow };
  if (!target) return;

  agentUsed = session;
  wanted = target;
  showDevice();
  void readDevices();
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
/// The devices each machine's node pushed on its `device-list` topic, by machine id. A machine not
/// here is read: its node has not answered yet, or predates the topic.
const announced = new Map<string, Device[]>();
// The desktop's `devices-changed` says a read of a machine that does not push changed.
const devicesChanged = ambient(readDevices, DEVICES_WAIT_MS, visible);
subscribeEach(topic("device-list"), event => {
  const heard = readAnnounced(event.data);
  if (heard === null) return;

  if (heard === "refused") announced.delete(event.machine_id);
  else announced.set(event.machine_id, heard);
  devicesChanged();
}, { when: visible });
// A node that predates `device-list` pings its older `devices` topic, and the machine is read.
subscribeChanges(topic("devices"), { deliver: "latest", receive: () => devicesChanged() }, { when: visible });
listen("devices-changed", () => devicesChanged());

function shownChanged() {
  if (!visible()) {
    clearTimeout(poll);
    detach();
    return;
  }

  // Shown again, a view that ended is tried again, and the session's
  // device is read again: another device may have changed it.
  if (deviceModel.getState().screen.kind === "ended") set({ screen: { kind: "off" } });
  kept = null;
  void readDevices();
}

/// The session the pick was made for; another one starts over.
let pickedFor: string | null = null;

/// Shows device `pick`; the session keeps it once it runs.
export function pickDevice(pick: DevicePick) {
  pickedFor = sessionKey(paneSession());
  wanted = null;
  if (pickedDevice(deviceModel.getState().machines, pick)?.state === "On") keep(pick);
  set({ pick, screen: { kind: "off" }, error: "" });
  follow();
}

// ----- The session's device -----

/// The device the pane's session keeps on its machine (`SessionInfo::device`),
/// as last read: the last one the tab showed running, or the agent used.
/// One its machine lists off or gone is forgotten.
/// Unknown until read; a read that failed is tried on the next pass.
let kept: { session: string; device: KeptDevice } | null = null;
/// A write is on its way, so the passes meanwhile send no other.
let keeping = false;

async function readKept(): Promise<DevicePick | null> {
  const session = paneSession(), key = sessionKey(session);
  if (!session) return null;
  if (kept?.session === key) return kept.device.kind === "kept" ? kept.device.pick : null;

  try {
    const device = readKeptDevice(await invoke("agent_device", { machineId: session.machine_id, agent: session.agent, session: session.id }));
    if (!device || sessionKey(paneSession()) !== key) return null;
    kept = { session: key, device };
    return device.kind === "kept" ? device.pick : null;
  } catch {
    return null;
  }
}

/// Has the pane's session keep `pick` as its device, or forget it. Nothing
/// goes until its device was read, nor to a machine too old to keep one;
/// the cache changes once the machine took it, so a failed write goes again
/// on the next pass.
function keep(pick: DevicePick | null) {
  const session = paneSession(), key = sessionKey(session);
  const known = kept?.session === key ? kept.device : null;
  if (!session || keeping || known?.kind !== "kept" || samePickOrNull(known.pick, pick)) return;
  keeping = true;
  invoke("agent_steer", { machineId: session.machine_id, agent: session.agent, session: session.id, steer: { device: { device: pick } } })
    .then(() => { if (kept?.session === key) kept = { session: key, device: { kind: "kept", pick } }; })
    .catch(() => { /* Tried again on the next pass. */ })
    .finally(() => { keeping = false; });
}

const samePick = (a: DevicePick, b: DevicePick) => a.machine === b.machine && a.device === b.device;
const samePickOrNull = (a: DevicePick | null, b: DevicePick | null) => a === b || (a !== null && b !== null && samePick(a, b));

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
    const value = await reads.run(Promise.all([listMachines(), readKept()]));
    if (value === STALE) return;
    const [machines, stored] = value;
    const at = paneCheckout(), platforms = sessionPlatforms(), session = sessionKey(paneSession());
    const last = session === pickedFor ? deviceModel.getState().pick : null;
    const asked = wanted ? targetPick(machines, wanted) : null;
    const known = keptPick(machines, stored);
    // The agent's node already keeps the device its calls used.
    if (asked && kept?.session === session && kept.device.kind === "kept") kept = { session, device: { kind: "kept", pick: asked } };
    const pick = asked ?? defaultPick(machines, at?.machine ?? null, last ?? known.pick, platforms);
    pickedFor = session;
    if (asked) wanted = null;
    // The agent's device shows again even when its view had ended.
    const screen = asked && deviceModel.getState().screen.kind === "ended" ? { screen: { kind: "off" } as const } : {};
    set({ machines, pick, platforms, listed: true, error: "", ...screen });
    if (pick && pickedDevice(machines, pick)?.state === "On") keep(pick);
    else if (known.forget) keep(null);
  } catch (e) {
    set({ listed: true, error: deviceError(e) });
  }

  follow();
  if (!visible()) return;
  const state = pickedDevice(deviceModel.getState().machines, deviceModel.getState().pick)?.state;
  if (state === "On" && live) return;
  // Machines whose nodes push their devices need no timer.
  if (hosts.every(host => announced.has(host.machine_id))) return;
  poll = setTimeout(() => void readDevices(), state === "Booting" ? BOOTING_POLL_MS : IDLE_POLL_MS);
}

/// Every host's devices: a machine that pushes its list gives it, the others are read.
async function listMachines(): Promise<DeviceMachine[]> {
  const read = await Promise.all(hosts.filter(host => !announced.has(host.machine_id))
    .map(host => invoke<unknown>("device_list", { machineId: host.machine_id }).then(readMachines)));
  const pushed = hosts.flatMap(host => {
    const devices = announced.get(host.machine_id);
    return devices ? [{ machine_id: host.machine_id, machine_name: host.machine_name, devices, error: null }] : [];
  });
  const all = [...read.flat(), ...pushed];
  return hosts.flatMap(host => all.filter(machine => machine.machine_id === host.machine_id));
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

  // A node that pushes its devices says when the boot or shut down ends.
  if (!announced.has(pick.machine)) await readDevices();
}

// ----- The live screen -----

/// Draws a frame; it owns the bitmap from then on.
type Painter = (frame: ImageBitmap) => void;

let painter: Painter | null = null;
/// The size of the box the screen draws in, for the frame size asked.
let sideFor: () => number = () => 0;

/// The view's canvas takes the frames, and says how large they should be.
export function setPainter(paint: Painter | null, side: () => number) {
  painter = paint;
  sideFor = side;
}

/// The attached device and the name of its view.
interface LiveStream { view: string; pick: DevicePick; video: boolean }
let live: LiveStream | null = null;

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

function attach(pick: DevicePick, id: string, preferVideo = true) {
  const view = crypto.randomUUID();
  const mine: LiveStream = { view, pick, video: preferVideo && videoSupported() };
  live = mine;
  queue = [];
  set({ screen: { kind: "attaching" } });
  const channel = indexedChannel(message => heard(mine, message));
  invoke("device_attach", { machineId: pick.machine, id, view, maxSide: sideFor(), video: mine.video, onEvent: channel })
    .then(() => { if (live === mine) ended(mine, "The view ended."); })
    .catch(e => {
      if (live !== mine) return;
      if (mine.video) fallbackVideo(mine);
      else ended(mine, deviceError(e));
    });
}

function detach() {
  if (!live) return;
  const { view } = live;
  live = null;
  stopVideo();
  queue = [];
  set({ screen: { kind: "off" } });
  invoke("device_detach", { view }).catch(() => {});
}

function ended(mine: LiveStream, reason: string) {
  if (live !== mine) return;
  live = null;
  stopVideo();
  set({ screen: { kind: "ended", reason } });
  // It may have stopped: the list says.
  void readDevices();
}

function heard(mine: LiveStream, message: unknown) {
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
    case "video":
      void startVideo(mine, read.fps);
      return;
    case "video_data":
      feedVideo(mine, read.data, read.rotation);
      return;
    case "frame":
      if (deviceModel.getState().media.kind !== "jpeg") stopVideo();
      void paint(mine, read.jpeg);
      return;
  }
}

function acknowledge(mine: LiveStream) {
  if (live === mine) invoke("device_drawn", { view: mine.view }).catch(() => {});
}

/// Decodes a frame and hands it to the painter, then asks for the next.
async function paint(mine: LiveStream, jpeg: Uint8Array<ArrayBuffer>) {
  try {
    const bitmap = await createImageBitmap(new Blob([jpeg], { type: "image/jpeg" }));
    if (live === mine && painter) painter(bitmap);
    else bitmap.close();
  } catch {
    // A frame that does not decode is skipped.
  } finally {
    acknowledge(mine);
  }
}

// H.264 stays encoded across the mesh. JMuxer packages it for the browser's
// native video decoder without encoding or decoding pixels itself.
let videoElement: HTMLVideoElement | null = null;
let video: { token: object; muxer: JMuxer | null; wait: ReturnType<typeof setTimeout> | null } | null = null;

export function setVideoElement(element: HTMLVideoElement | null) {
  if (!element) stopVideo();
  videoElement = element;
}

declare global {
  interface Window { ManagedMediaSource?: typeof MediaSource }
}

function videoSupported(): boolean {
  const source = typeof MediaSource === "undefined" ? window.ManagedMediaSource ?? null : MediaSource;
  return videoElement !== null && source !== null
    && ["avc1.42E01E", "avc1.4D402A", "avc1.64002A"].some(codec => source.isTypeSupported(`video/mp4; codecs="${codec}"`));
}

function clearVideoWait() {
  if (video?.wait != null) { clearTimeout(video.wait); video.wait = null; }
}

async function startVideo(mine: LiveStream, fps: number) {
  try {
    const { default: Muxer } = await import("jmuxer");
    if (live !== mine || !videoElement) return;
    stopVideo();
    const token = {};
    video = { token, muxer: null, wait: null };
    const failed = () => { if (video?.token === token) fallbackVideo(mine); };
    const options: JMuxer.Options & { onUnsupportedCodec: () => void } = {
      node: videoElement, mode: "video", fps, readFpsFromTrack: true,
      flushingTime: Math.max(1, Math.floor(1000 / fps)), maxDelay: 100, clearBuffer: true,
      onReady: () => {
        if (video?.token !== token) return;
        acknowledge(mine);
        videoElement?.play().catch(failed);
      },
      onError: failed, onUnsupportedCodec: failed,
    };
    const instance = new Muxer(options);
    if (live !== mine || video?.token !== token) { instance.destroy(); return; }
    video.muxer = instance;
    video.wait = setTimeout(failed, 5000);
    set({ media: { kind: "h264", width: 0, height: 0, rotation: 0 } });
  } catch {
    fallbackVideo(mine);
  }
}

function feedVideo(mine: LiveStream, data: Uint8Array<ArrayBuffer>, rotation: VideoRotation) {
  if (!video?.muxer || live !== mine) return;
  try {
    const media = deviceModel.getState().media;
    if (media.kind === "h264" && media.rotation !== rotation) set({ media: { ...media, rotation } });
    video.muxer.feed({ video: data });
    acknowledge(mine);
  } catch {
    fallbackVideo(mine);
  }
}

export function videoSizeChanged() {
  const media = deviceModel.getState().media;
  if (!videoElement || media.kind !== "h264") return;
  set({ media: { ...media, width: videoElement.videoWidth, height: videoElement.videoHeight } });
}

export function videoPlaying() {
  clearVideoWait();
  videoSizeChanged();
}

export function videoFailed() {
  if (live && deviceModel.getState().media.kind === "h264") fallbackVideo(live);
}

function stopVideo() {
  clearVideoWait();
  const old = video?.muxer ?? null;
  video = null;
  if (deviceModel.getState().media.kind !== "jpeg") set({ media: { kind: "jpeg" } });
  old?.destroy();
}

function fallbackVideo(mine: LiveStream) {
  if (live !== mine || !mine.video) return;
  detach();
  if (visible()) attach(mine.pick, mine.pick.device, false);
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

/// Turns the Device tab's dock to it, unfolded, without taking the keys
/// from the conversation. A phone keeps its screen.
function showDevice() {
  if (!shell() || paneModel.getState().phone || !hasDeviceTab()) return;
  const d = dockOf(DEVICE);
  requestRightPane(d);
  setFolded(d, false);
  saveLayout();
  setOn(d, DEVICE);
  rememberPane();
  redraw();
}
