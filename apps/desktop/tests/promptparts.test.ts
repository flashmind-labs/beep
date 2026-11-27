import { test } from "node:test";
import assert from "node:assert/strict";
import { promptParts, sessionMessageTitle } from "../src/stores/agents/promptparts-state.ts";

test("a tagged prompt splits into Beep's context, another session and the user", () => {
  const text = `<beep-context>\nBeep: the tools.\n</beep-context>\n<from-session id="p1" name="Fix &quot;it&quot;">\nRun the tests.\n</from-session>\nok, let's keep going`;
  assert.deepEqual(promptParts(text), [
    { kind: "context", text: "Beep: the tools." },
    { kind: "session", id: "p1", name: 'Fix "it"', text: "Run the tests." },
    { kind: "user", text: "ok, let's keep going" },
  ]);
});

test("a child's note is a part of its own", () => {
  const text = `<beep-child id="k1@box" name="Fix &lt;it&gt;" state="asking">\n[child k1@box is waiting on the user] Run it?\nOnly the user can answer this.\n</beep-child>`;
  assert.deepEqual(promptParts(text), [
    { kind: "child", id: "k1@box", name: "Fix <it>", state: "asking", text: "[child k1@box is waiting on the user] Run it?\nOnly the user can answer this." },
  ]);
});

test("untagged text is the user's", () => {
  assert.deepEqual(promptParts("a <b> c <beep-contextual>"), [{ kind: "user", text: "a <b> c <beep-contextual>" }]);
});

test("session message titles unwrap imported prompts without showing protocol tags", () => {
  assert.equal(sessionMessageTitle('  Review the tests  '), 'Review the tests');
  assert.equal(sessionMessageTitle('<from-session id="source" name="Review &quot;login&quot;">Hello</from-session>'), 'Review "login"');
  assert.equal(sessionMessageTitle('<beep-child id="child" name="Check tests" state="finished">Passed</beep-child>'), 'Check tests');
  assert.equal(sessionMessageTitle('<from-session id="source" name="Review'), '');
  assert.equal(sessionMessageTitle('<beep-context>Tools</beep-context>'), '');
  assert.equal(sessionMessageTitle('<from-session id="source">Hello</from-session>'), '');
});

test("Claude Code's own messages from sessions and agents are another session's", () => {
  const text = `<cross-session-message from="uds:/srv/one.sock" from-name="build-42" from-mode="bypass">\nRebase first.\n</cross-session-message>\n<agent-message from="a1">\nDone.\n</agent-message>\nthanks`;
  assert.deepEqual(promptParts(text), [
    { kind: "session", id: "uds:/srv/one.sock", name: "build-42", text: "Rebase first." },
    { kind: "session", id: "a1", name: "", text: "Done." },
    { kind: "user", text: "thanks" },
  ]);
});
