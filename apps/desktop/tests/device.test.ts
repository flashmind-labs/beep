import test from "node:test";
import assert from "node:assert/strict";
import {
  type DeviceInput, type DeviceMachine, buttonsFor, defaultPick, fitRect, keyInput, maxSide, pickValue, queueInput,
  readMachines, readPickValue, readScreenMessage, touchPoint,
} from "../src/agentpane/device-state.ts";

const device = (id: string, state: "Off" | "Booting" | "On" = "Off") => ({ id, name: id, platform: "Android" as const, state, os: null });
const machines: DeviceMachine[] = [
  { machine_id: "a", machine_name: "build-box", devices: [], error: null },
  { machine_id: "b", machine_name: "gpu-box", devices: [device("pixel"), device("tablet", "On")], error: null },
  { machine_id: "c", machine_name: "mac-box", devices: [device("phone")], error: null },
];

test("the pick starts on the session's machine, a running device first", () => {
  assert.deepEqual(defaultPick(machines, "c", null), { machine: "c", device: "phone" });
  assert.deepEqual(defaultPick(machines, "a", null), { machine: "b", device: "tablet" });
  assert.deepEqual(defaultPick(machines, null, { machine: "b", device: "pixel" }), { machine: "b", device: "pixel" });
  assert.deepEqual(defaultPick(machines, "c", { machine: "b", device: "gone" }), { machine: "c", device: "phone" });
  assert.equal(defaultPick([machines[0]], "a", null), null);
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
  assert.ok(buttonsFor("Android").includes("Back"));
  assert.ok(!buttonsFor("Ios").includes("AppSwitch"));
});
