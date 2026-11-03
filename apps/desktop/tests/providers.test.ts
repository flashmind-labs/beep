import test from "node:test";
import assert from "node:assert/strict";
import { checkedLabel, draftChanged, draftOf, envProblem, hasUpdate, isNewer, joinArgs, listRows, settingsInput, splitArgs, statusLine, summaryLine, toggledInput, updatable, versionLine, type HarnessView, type HarnessesView } from "../src/providers-state.ts";

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

test("rows list each harness with its version, update marker, status and switch", () => {
  const rows = listRows(view);
  assert.deepEqual(rows.map(r => r.agent), ["claude", "codex", "pi", "gemini", "flsh"]);
  const [claude, codex, pi, gemini] = rows;
  assert.equal(claude.name, "Claude Code");
  assert.equal(claude.initials, "CC");
  assert.equal(claude.update, true);
  assert.equal(claude.status, "Signed in · Max plan");
  assert.equal(codex.update, false);
  assert.deepEqual([codex.status, codex.bad], ["Not signed in on this machine", true]);
  assert.equal(pi.on, false);
  assert.match(pi.status, /Turned off/);
  assert.deepEqual([gemini.status, gemini.switchable, gemini.version], ["Not installed", false, null]);
  // OpenCode shows once the machine has it.
  const withOpenCode = { ...view, harnesses: view.harnesses.map(h => h.agent === "opencode" ? { ...h, installed: true } : h) };
  assert.equal(listRows(withOpenCode).length, 6);
});

test("status lines say how each harness signs in", () => {
  assert.equal(statusLine(harness("claude", { sign_in: { state: "beep_token", account: null, plan: null, last4: "7d1e" } })).text, "Signs in with Beep's token …7d1e");
  assert.equal(statusLine(harness("flsh")).text, "Installed");
  assert.deepEqual(statusLine(harness("pi", { runnable: false })), { text: "Installed; Beep cannot start it yet", bad: true });
});

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

test("the check button says how long ago the versions were looked up", () => {
  const now = 1_760_000_000_000;
  assert.equal(checkedLabel(null, now), "Check for updates");
  assert.equal(checkedLabel(now - 20_000, now), "Checked just now");
  assert.equal(checkedLabel(now - 60_000, now), "Checked 1 min ago");
  assert.equal(checkedLabel(now - 3 * 3_600_000, now), "Checked 3 h ago");
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

test("the summary line names the signed-in account, else says what the list says", () => {
  const [claude, codex, pi, gemini] = view.harnesses;
  assert.deepEqual(summaryLine(claude), { text: "Signed in as you@example.com · Max plan", bad: false });
  assert.deepEqual(summaryLine(codex), statusLine(codex));
  assert.deepEqual(summaryLine(pi), statusLine(pi));
  assert.deepEqual(summaryLine(gemini), { text: "Not installed", bad: false });
});
