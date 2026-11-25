import test from "node:test";
import assert from "node:assert/strict";
import type { AgentToolCall } from "../src/agents/agents-state.ts";
import { commandText, isCommandCall, resultText } from "../src/agents/tool-details-state.ts";
import { toolDetails } from "../src/agents/tool-details.ts";

const call = (changes: Partial<AgentToolCall> = {}): AgentToolCall => ({
  name: "Bash", input: '{"command":"sleep 45","description":"Timer to recheck the build"}', input_truncated: false,
  result: null, result_truncated: false, ...changes,
});

test("the actual command comes from arguments rather than the description", () => {
  assert.equal(commandText(call()), "sleep 45");
  assert.equal(commandText(call({ input: '{"description":"Timer to recheck the build"}' })), null);
  assert.equal(commandText(call({ input: '{"cmd":"cargo check"}' })), "cargo check");
  assert.equal(commandText(call({ input: '{"command":["/bin/sh","-c","echo hello"]}' })), '[\n  "/bin/sh",\n  "-c",\n  "echo hello"\n]');
  for (const input of ['null', '[]', '{', '{"command":42}']) assert.equal(commandText(call({ input })), null);
  assert.equal(commandText(call({ input_truncated: true })), null);
});

test("only shell calls get the command inspector", () => {
  assert.equal(isCommandCall(call()), true);
  assert.equal(isCommandCall(call({ name: "exec_command" })), true);
  assert.equal(isCommandCall(call({ name: null })), true);
  assert.equal(isCommandCall(call({ name: null, input: null })), false);

  for (const name of ['Read', 'Grep', 'Edit', 'mcp__beep__remote_run', 'mcp__clock__sleep']) {
    assert.equal(isCommandCall(call({ name })), false);
  }

  assert.equal(isCommandCall(null), false);
});

test("expanded commands show the tool, command and output", () => {
  assert.deepEqual(toolDetails(call(), "in_progress", true, ""), { tool: "Bash", truncated: false, command: "sleep 45", outputTruncated: false, output: { kind: "text", text: "No output yet." } });
  const absent = toolDetails(null, "", false, "");
  assert.equal(absent.tool, "Tool name unavailable");
  assert.equal(absent.command, "Command unavailable.");
  assert.deepEqual(absent.output, { kind: "text", text: "No output recorded." });
  const truncated = toolDetails(call({ input_truncated: true }), "completed", false, "");
  assert.equal(truncated.truncated, true);
  assert.ok(truncated.command.includes('"command"'));
});

test("output from historical calls is readable and live output keeps priority", () => {
  assert.equal(resultText(call({ result: '[{"type":"text","text":"done"}]' })), 'done');
  assert.equal(resultText(call({ result: '"done"' })), 'done');
  assert.ok(resultText(call({ result: '{"exit_code":0}' })).includes('"exit_code": 0'));
  assert.equal(resultText(call({ result: 'truncated', result_truncated: true })), 'truncated');
  assert.deepEqual(toolDetails(call({ result: '"old result"' }), "completed", false, 'latest output').output, { kind: "text", text: "latest output" });
});
