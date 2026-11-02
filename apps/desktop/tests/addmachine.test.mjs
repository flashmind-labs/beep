import test from "node:test";
import assert from "node:assert/strict";
import { START, next, timeLeft, machineById, describeMachine, outdated, macOption } from "../dist/addmachine-state.js";

const created = { type: "created", token: "bk1-x", command: "beep-node setup bk1-x", expiresAt: 600_000 };

test("the countdown reads minutes and seconds and stops at zero", () => {
  assert.equal(timeLeft(600_000, 0), "10:00");
  assert.equal(timeLeft(600_000, 18_500), "9:42");
  assert.equal(timeLeft(600_000, 599_001), "0:01");
  assert.equal(timeLeft(600_000, 700_000), "0:00");
});

test("a token waits, then expires on the tick that reaches it", () => {
  let s = next(START, created);
  assert.equal(s.step, "waiting");
  assert.equal(s.command, "beep-node setup bk1-x");
  assert.equal(next(s, { type: "tick", now: 599_999 }), s);
  s = next(s, { type: "tick", now: 600_000 });
  assert.deepEqual(s, { step: "expired" });
  assert.deepEqual(next(s, { type: "renew" }), START);
});

test("a found machine links only while waiting", () => {
  const machine = { id: "m", name: "build-box" };
  const s = next(next(START, created), { type: "found", machine });
  assert.deepEqual(s, { step: "linked", machine });
  // Neither a later tick nor a late token undoes it.
  assert.equal(next(s, { type: "tick", now: 10_000_000 }), s);
  assert.equal(next(s, created), s);
  assert.equal(next({ step: "expired" }, { type: "found", machine }).step, "expired");
});

test("an old server reads as needing an update", () => {
  const msg = "The server needs an update before machines can be added from here.";
  assert.equal(next(START, { type: "failed", message: msg }).step, "outdated");
  assert.equal(next(START, { type: "failed", message: "unauthorized" }).step, "error");
  assert.ok(!outdated(undefined));
});

test("the linked machine is the one that claimed the token, even one already listed", () => {
  const machines = [{ id: "a", name: "old box" }, { id: "c", name: "new" }];
  assert.equal(machineById(machines, "a").name, "old box");
  assert.equal(machineById(machines, "z"), null);
  assert.equal(machineById(machines, null), null);
  assert.equal(machineById(null, "a"), null);
});

test("a machine reads as its OS and known tools", () => {
  const env = { os: "linux", tools: [{ name: "git" }, { name: "docker" }, { name: "claude" }, { name: "codex" }] };
  assert.equal(describeMachine({ env }), "Linux, Docker, Claude Code, Codex");
  assert.equal(describeMachine({ env: { os: "macos", tools: [{ name: "opencode" }] } }), "macOS, OpenCode");
  assert.equal(describeMachine({ env: null }), "");
});

test("Use this Mac is offered while the Mac is off and follows it once asked", () => {
  const mac = (over = {}) => ({ id: "mac-key", name: "Studio", enabled: true, status: "connecting", error: null, ...over });
  assert.equal(macOption(null, false), null);
  assert.equal(macOption(mac({ status: "unlinked" }), false), null);
  assert.equal(macOption(mac({ status: "connected" }), false), null);
  const off = macOption(mac({ enabled: false, status: "off" }), false);
  assert.equal(off.button, "Use this Mac");
  assert.equal(macOption(mac(), true).busy, true);
  assert.equal(macOption(mac({ status: "connected" }), true).done, true);
  assert.match(macOption(mac({ status: "failed", error: "connect timed out" }), true).text, /connect timed out/);
});
