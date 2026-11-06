import { test } from "node:test";
import assert from "node:assert/strict";
import { promptParts } from "../src/agents/promptparts-state.ts";

test("a tagged prompt splits into Beep's context, another session and the user", () => {
  const text = `<beep-context>\nBeep: the tools.\n</beep-context>\n<from-session id="p1" name="Fix &quot;it&quot;">\nRun the tests.\n</from-session>\nok, let's keep going`;
  assert.deepEqual(promptParts(text), [
    { kind: "context", text: "Beep: the tools." },
    { kind: "session", id: "p1", name: 'Fix "it"', text: "Run the tests." },
    { kind: "user", text: "ok, let's keep going" },
  ]);
});

test("untagged text is the user's", () => {
  assert.deepEqual(promptParts("a <b> c <beep-contextual>"), [{ kind: "user", text: "a <b> c <beep-contextual>" }]);
});
