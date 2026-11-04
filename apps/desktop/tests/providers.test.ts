import test from "node:test";
import assert from "node:assert/strict";
import { draftChanged, draftOf, envProblem, hasUpdate, isNewer, joinArgs, settingsInput, splitArgs, toggledInput, updatable, versionLine, type HarnessView, type HarnessesView } from "../src/settings/providers-state.ts";

const settings = { enabled: true, program: "", home: "", args: [], env: [] };
const harness = (agent: string, more: Partial<HarnessView> = {}): HarnessView => ({
  agent, installed: true, runnable: true, version: "1.0.0", latest: "1.0.0", program: `/usr/bin/${agent}`,
  home_var: null, home_default: null, sign_in: { state: "unknown", account: null, plan: null, last4: null },
  install: `npm install -g ${agent}@latest`, settings, ...more,
});
const view: HarnessesView = {
  machine_id: "m1", machine_name: "build-box", checked_at: 0,
  harnesses: [
    harness("claude", { version: "2.1.312", latest: "2.1.320", sign_in: { state: "signed_in", account: "you@example.com", plan: "max", last4: null } }),
    harness("codex", { version: "0.166.0", latest: "0.166.0", sign_in: { state: "signed_out", account: null, plan: null, last4: null } }),
    harness("pi", { settings: { ...settings, enabled: false } }),
    harness("gemini", { installed: false, runnable: false, version: null, latest: null }),
    harness("flsh", { latest: null }),
    harness("opencode", { installed: false, runnable: false, version: null }),
  ],
};

test("versions compare by number and show what is out", () => {
  assert.ok(isNewer("0.10.0", "0.9.9"));
  assert.ok(!isNewer("2.1.312", "2.1.312"));
  assert.ok(!isNewer("1.2.0-beta", "1.2.0"));
  assert.equal(versionLine(view.harnesses[0]), "v2.1.312 · v2.1.320 out");
  assert.equal(versionLine(view.harnesses[1]), "v0.166.0");
  assert.equal(versionLine(view.harnesses[3]), "");
  assert.ok(!hasUpdate(harness("flsh", { latest: null })));
  assert.deepEqual(updatable(view), ["claude"]);
  assert.deepEqual(updatable({ ...view, harnesses: [harness("claude", { latest: "9.0.0", install: null })] }), []);
});

test("launch arguments split and join like a shell line", () => {
  assert.deepEqual(splitArgs(`--profile work --name 'two words' "" -v`), ["--profile", "work", "--name", "two words", "", "-v"]);
  assert.deepEqual(splitArgs("   "), []);
  const args = ["--name", "two words", "it's"];
  assert.deepEqual(splitArgs(joinArgs(args)), args);
});

test("saving sends typed values only, and the switch keeps the saved ones", () => {
  const saved = harness("codex", { settings: { enabled: true, program: "", home: "~/.codex", args: ["-v"], env: ["HTTPS_PROXY"] } });
  const draft = draftOf(saved);
  assert.equal(draftChanged(saved, draft), false);
  draft.env.push({ name: "NEW_VAR", value: "x", saved: false });
  draft.args = "-v --fast";
  assert.equal(draftChanged(saved, draft), true);
  assert.deepEqual(settingsInput(true, draft), {
    enabled: true, program: "", home: "~/.codex", args: ["-v", "--fast"],
    env: [{ name: "HTTPS_PROXY", value: null }, { name: "NEW_VAR", value: "x" }],
  });
  assert.deepEqual(toggledInput(saved, false).env, [{ name: "HTTPS_PROXY", value: null }]);
  assert.equal(toggledInput(saved, false).enabled, false);
  assert.equal(envProblem([{ name: "1X", value: "", saved: false }]), "1X is not a variable name.");
  assert.equal(envProblem([{ name: "A", value: "", saved: false }, { name: "A", value: "", saved: true }]), "A is set twice.");
  assert.equal(envProblem([{ name: "", value: "", saved: false }]), "");
});
