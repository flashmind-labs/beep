import test from "node:test";
import assert from "node:assert/strict";
import type { AgentToolCall } from "../src/agents/agents.ts";
import { commandText, isCommandCall, resultText } from "../src/agents/tool-details-state.ts";
import { toolDetailsHtml } from "../src/agents/tool-details.ts";

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

test("expanded commands show the tool, command and output without timer UI", () => {
  const html = toolDetailsHtml(call(), "in_progress", true, "");
  assert.ok(html.includes('<dd>Bash</dd>'));
  assert.ok(html.includes('<h4>Command</h4><pre>sleep 45</pre>'));
  assert.ok(html.includes('No output yet.'));
  assert.ok(!html.includes('Requested delay'));
  assert.ok(!html.includes('Expected wake'));
  assert.ok(!html.includes('Waiting'));
  assert.ok(!html.includes('<h4>Arguments</h4>'));
});

test("arguments and output are escaped and missing or truncated input is explicit", () => {
  const html = toolDetailsHtml(call({ name: '<script>bad()</script>', input: '{"command":"echo <img src=x>"}' }), "completed", false, '<script>output()</script>');
  assert.ok(html.includes('&lt;script&gt;bad()&lt;/script&gt;'));
  assert.ok(html.includes('echo &lt;img src=x&gt;'));
  assert.ok(html.includes('&lt;script&gt;output()&lt;/script&gt;'));
  assert.ok(!html.includes('<script>'));
  const absent = toolDetailsHtml(null, "", false, "");
  assert.ok(absent.includes('Tool name unavailable'));
  assert.ok(absent.includes('Command unavailable.'));
  assert.ok(absent.includes('No output recorded.'));
  const truncated = toolDetailsHtml(call({ input_truncated: true }), "completed", false, "");
  assert.ok(truncated.includes('Arguments (truncated)'));
  assert.ok(truncated.includes('&quot;command&quot;'));
});

test("output from historical calls is readable and live output keeps priority", () => {
  assert.equal(resultText(call({ result: '[{"type":"text","text":"done"}]' })), 'done');
  assert.equal(resultText(call({ result: '"done"' })), 'done');
  assert.ok(resultText(call({ result: '{"exit_code":0}' })).includes('"exit_code": 0'));
  assert.equal(resultText(call({ result: 'truncated', result_truncated: true })), 'truncated');
  const html = toolDetailsHtml(call({ result: '"old result"' }), "completed", false, 'latest output');
  assert.ok(html.includes('latest output'));
  assert.ok(!html.includes('old result'));
});
