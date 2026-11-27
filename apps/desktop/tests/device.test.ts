import test from "node:test";
import assert from "node:assert/strict";
import {
  type DeviceInput, type DeviceMachine, buttonsFor, defaultPick, fitRect, rotatedSize, keyInput, maxSide, missingLine, pickValue, queueInput,
  keptPick, readAnnounced, readKeptDevice, readMachines, readPickValue, readPlatforms, readScreenMessage, touchPoint, commandName, deviceCall, followedDevice, targetPick,
} from "../src/stores/agentpane/device-state.ts";

const device = (id: string, state: "Off" | "Booting" | "On" = "Off") => ({ id, name: id, platform: "Android" as const, state, os: null, phone: false });
const machines: DeviceMachine[] = [
  { machine_id: "a", machine_name: "build-box", devices: [], error: null },
  { machine_id: "b", machine_name: "gpu-box", devices: [device("pixel"), device("tablet", "On")], error: null },
  { machine_id: "c", machine_name: "mac-box", devices: [device("phone")], error: null },
];

test("the pick starts on the session's machine, a running device first", () => {
  const android = ["Android" as const];
  assert.deepEqual(defaultPick(machines, "c", null, android), { machine: "c", device: "phone" });
  assert.deepEqual(defaultPick(machines, "a", null, android), { machine: "b", device: "tablet" });
  assert.deepEqual(defaultPick(machines, null, { machine: "b", device: "pixel" }, android), { machine: "b", device: "pixel" });
  assert.deepEqual(defaultPick(machines, "c", { machine: "b", device: "gone" }, android), { machine: "c", device: "phone" });
  assert.equal(defaultPick([machines[0]], "a", null, android), null);
});

const simulator = (id: string, state: "Off" | "On" = "Off") => ({ id, name: id, platform: "Ios" as const, state, os: null, phone: false });
const mixed: DeviceMachine[] = [
  { machine_id: "linux", machine_name: "build-box", devices: [device("pixel", "On")], error: null },
  { machine_id: "mac", machine_name: "mac-box", devices: [device("pixel-mac"), simulator("iphone"), simulator("ipad", "On")], error: null },
];

test("the pick is a device of the project's platform, wherever it is", () => {
  // An iOS project on a Linux box starts on the Mac's running simulator.
  assert.deepEqual(defaultPick(mixed, "linux", null, ["Ios"]), { machine: "mac", device: "ipad" });
  // An Android project stays on its own machine.
  assert.deepEqual(defaultPick(mixed, "mac", null, ["Android"]), { machine: "mac", device: "pixel-mac" });
  // Both platforms: the session's machine first.
  assert.deepEqual(defaultPick(mixed, "linux", null, ["Android", "Ios"]), { machine: "linux", device: "pixel" });
  // No device of the platform: nothing picked.
  assert.equal(defaultPick([mixed[0]], "linux", null, ["Ios"]), null);
  assert.equal(defaultPick(mixed, "linux", null, []), null);
});

test("a platform no machine has a device for gets one line", () => {
  assert.equal(missingLine(mixed, ["Android", "Ios"]), null);
  assert.equal(missingLine([mixed[0]], ["Android", "Ios"]), "No machine has an iOS simulator. A Mac with Xcode and idb_companion can run one.");
  assert.equal(missingLine([], ["Android"]), "No machine has an Android emulator. Android Studio can make one.");
  assert.equal(missingLine([], []), null);
});

test("platforms read from the node's answer, unknown ones left out", () => {
  assert.deepEqual(readPlatforms(["Android", "Ios", "Web"]), ["Android", "Ios"]);
  assert.deepEqual(readPlatforms(null), []);
});

test("a pick goes through the picker's value and back", () => {
  const pick = { machine: "b", device: "emulator-5554" };
  assert.deepEqual(readPickValue(pickValue(pick)), pick);
  assert.equal(readPickValue(""), null);
});

test("the screen fits the view with its aspect kept, letterboxed", () => {
  assert.deepEqual(fitRect({ width: 400, height: 400 }, { width: 100, height: 200 }), { x: 100, y: 0, width: 200, height: 400 });
  assert.deepEqual(fitRect({ width: 0, height: 400 }, { width: 100, height: 200 }), { x: 0, y: 0, width: 0, height: 0 });
});

test("a touch is a fraction of the drawn image, not of the letterbox", () => {
  const rect = { x: 100, y: 0, width: 200, height: 400 };
  assert.deepEqual(touchPoint(rect, 200, 100, false), { x: 0.5, y: 0.25 });
  assert.equal(touchPoint(rect, 50, 100, false), null);
  assert.deepEqual(touchPoint(rect, 50, 500, true), { x: 0, y: 1 });
});

test("frames are asked at the view's pixel size, in steps and bounds", () => {
  assert.equal(maxSide({ width: 300, height: 500 }, 2), 1024);
  assert.equal(maxSide({ width: 10, height: 10 }, 1), 160);
  assert.equal(maxSide({ width: 3000, height: 3000 }, 2), 2048);
});

test("special keys go as keys; text and shortcuts do not", () => {
  const press = (key: string, mods: Partial<{ metaKey: boolean; ctrlKey: boolean; altKey: boolean }> = {}) =>
    keyInput({ key, metaKey: false, ctrlKey: false, altKey: false, isComposing: false, ...mods });
  assert.deepEqual(press("Enter"), { Key: { key: "Return" } });
  assert.deepEqual(press("Backspace"), { Key: { key: "Delete" } });
  assert.deepEqual(press("ArrowUp"), { Key: { key: "Up" } });
  assert.equal(press("a"), null);
  assert.equal(press("Enter", { metaKey: true }), null);
});

test("a move waiting last is replaced by the next move", () => {
  const move = (x: number): DeviceInput => ({ Touch: { phase: "Move", x, y: 0 } });
  const down: DeviceInput = { Touch: { phase: "Down", x: 0, y: 0 } };
  assert.deepEqual(queueInput(queueInput([down], move(0.1)), move(0.2)), [down, move(0.2)]);
  assert.deepEqual(queueInput([move(0.1)], down), [move(0.1), down]);
});

test("frames read as their size and JPEG, notices as JSON", () => {
  const bytes = new Uint8Array([2, 0, 0, 0, 4, 0, 0, 0, 0xff, 0xd8]);
  const frame = readScreenMessage(bytes.buffer);
  assert.equal(frame?.kind, "frame");
  if (frame?.kind === "frame") {
    assert.deepEqual([frame.width, frame.height, [...frame.jpeg]], [2, 4, [0xff, 0xd8]]);
  }
  assert.equal(readScreenMessage([...bytes])?.kind, "frame");
  assert.deepEqual(readScreenMessage({ kind: "ended", reason: "The emulator closed." }), { kind: "ended", reason: "The emulator closed." });
  assert.equal(readScreenMessage({ kind: "other" }), null);
});

test("device lists are checked, and only Android has Back and Recent apps", () => {
  const read = readMachines([{ machine_id: "a", machine_name: "x", devices: [{ id: "p", name: "Pixel", platform: "Android", state: "On", os: "Android 16" }, { id: "q" }], error: null }, 7]);
  assert.equal(read.length, 1);
  assert.equal(read[0].devices.length, 1);
  const device = (platform: "Android" | "Ios", phone: boolean) => ({ id: "d", name: "d", platform, state: "On" as const, os: null, phone });
  assert.ok(buttonsFor(device("Android", false)).includes("Back"));
  assert.ok(!buttonsFor(device("Ios", false)).includes("AppSwitch"));
  assert.deepEqual(buttonsFor(device("Ios", true)), []);
});

const tool = (text: string) => ({ kind: "tool", text });

test("a device call names its device and machine", () => {
  assert.deepEqual(deviceCall("Beep: Boot iPhone 17 on mac-box"), { verb: "Boot", device: "iPhone 17", machine: "mac-box", detail: "" });
  assert.deepEqual(deviceCall("Beep: Press home on Pixel 9 on build-box"), { verb: "Press", device: "Pixel 9", machine: "build-box", detail: "home" });
  assert.equal(deviceCall("Beep: Type into Pixel 9 on build-box: turn on wifi")?.detail, "turn on wifi");
  assert.equal(deviceCall("Beep: Boot a device on a machine"), null);
  assert.equal(deviceCall("Beep: Run on build-box: make"), null);
});

test("the tab follows device calls that finish after the view opened, not history", () => {
  const history = [tool("Beep: Boot Pixel 9 on build-box [completed]"), tool("Beep: Tap Pixel 9 on build-box: 1, 2 [in_progress]")];
  const first = followedDevice(history, 10, null);
  assert.equal(first.target, null);
  assert.ok(first.follow);

  // The call that ran when the view opened finishes; a failed one and a shutdown do not move the tab.
  const later = [tool("Beep: Boot Pixel 9 on build-box [completed]"), tool("Beep: Tap Pixel 9 on build-box: 1, 2 [completed]")];
  const tapped = followedDevice(later, 10, first.follow);
  assert.deepEqual(tapped.target, { machine: "build-box", device: "Pixel 9" });
  assert.equal(followedDevice(later, 10, tapped.follow).target, null);

  const more = [...later, tool("Beep: Show iPhone 17 on mac-box (failed)"), tool("Beep: Shut down Pixel 9 on build-box [completed]"), tool("Run make [completed]")];
  assert.equal(followedDevice(more, 10, tapped.follow).target, null);

  // Older pages loaded above the first one are history too.
  const older = [tool("Beep: Show iPhone 17 on mac-box [completed]"), ...later];
  assert.equal(followedDevice(older, 9, tapped.follow).target, null);

  // A log not read yet waits for its first entries.
  assert.deepEqual(followedDevice([], 0, null), { follow: null, target: null });
});

test("a device call's target is found by machine name and device id or name", () => {
  const listed: DeviceMachine[] = [
    { machine_id: "k1", machine_name: "Mac Box", devices: [simulator("UDID-1"), { ...simulator("UDID-2"), name: "iPhone 17" }], error: null },
  ];
  assert.equal(commandName("Sam's Mac Box"), "sams-mac-box");
  assert.deepEqual(targetPick(listed, { machine: "mac-box", device: "UDID-1" }), { machine: "k1", device: "UDID-1" });
  assert.deepEqual(targetPick(listed, { machine: "mac-box", device: "iphone 17" }), { machine: "k1", device: "UDID-2" });
  assert.equal(targetPick(listed, { machine: "gpu-box", device: "UDID-1" }), null);
  assert.equal(targetPick(listed, { machine: "mac-box", device: "Pixel" }), null);
});

test("a session's kept device shows while it runs and is forgotten once off or gone", () => {
  const listed: DeviceMachine[] = [
    ...machines,
    { machine_id: "d", machine_name: "old-box", devices: [], error: "beep-node there is too old for devices." },
  ];
  assert.deepEqual(keptPick(listed, { machine: "b", device: "tablet" }), { pick: { machine: "b", device: "tablet" }, forget: false });
  assert.deepEqual(keptPick(listed, { machine: "b", device: "pixel" }), { pick: null, forget: true });
  assert.deepEqual(keptPick(listed, { machine: "b", device: "gone" }), { pick: null, forget: true });
  // An offline machine, or one that could not list, leaves it be.
  assert.deepEqual(keptPick(listed, { machine: "z", device: "pixel" }), { pick: null, forget: false });
  assert.deepEqual(keptPick(listed, { machine: "d", device: "pixel" }), { pick: null, forget: false });
  assert.deepEqual(keptPick(listed, null), { pick: null, forget: false });
  assert.deepEqual(readKeptDevice({ kind: "kept", pick: { machine: "b", device: "tablet" } }), { kind: "kept", pick: { machine: "b", device: "tablet" } });
  assert.deepEqual(readKeptDevice({ kind: "kept", pick: null }), { kind: "kept", pick: null });
  assert.deepEqual(readKeptDevice({ kind: "unsupported" }), { kind: "unsupported" });
  assert.equal(readKeptDevice({ kind: "kept", pick: { machine: 1 } }), null);
});

test("video chunks preserve native bytes and reject invalid rotation and size", () => {
  const bytes = new Uint8Array([0x56, 0x49, 0x44, 0x31, 1, 0, 0, 0, 0, 0, 1, 0x65]);
  const chunk = readScreenMessage(bytes.buffer);
  assert.equal(chunk?.kind, "video_data");
  if (chunk?.kind === "video_data") {
    assert.equal(chunk.rotation, 1);
    assert.deepEqual([...chunk.data], [0, 0, 1, 0x65]);
  }
  bytes[4] = 4;
  assert.equal(readScreenMessage(bytes.buffer), null);
  assert.equal(readScreenMessage({ kind: "video", fps: -1 }), null);
  assert.deepEqual(readScreenMessage({ kind: "video", fps: 60 }), { kind: "video", fps: 60 });
});

test("quarter turns swap the displayed video dimensions", () => {
  assert.deepEqual(rotatedSize({ width: 100, height: 200 }, 1), { width: 200, height: 100 });
  assert.deepEqual(rotatedSize({ width: 100, height: 200 }, 2), { width: 100, height: 200 });
});

test("a node's pushed devices read as its list, and the error of a node without the topic as refused", () => {
  const pushed = readAnnounced({ Devices: [{ id: "p", name: "Pixel", platform: "Android", state: "On", os: null, phone: true }, { id: "q" }] });
  assert.deepEqual(pushed, [{ id: "p", name: "Pixel", platform: "Android", state: "On", os: null, phone: true }]);
  assert.equal(readAnnounced({ error: "no state has been published for this topic" }), "refused");
  assert.equal(readAnnounced({ Terminals: [] }), null);
  assert.equal(readAnnounced(null), null);
});
