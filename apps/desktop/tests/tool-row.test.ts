import { bar } from "./dom-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { AG, type AgentPage, type AgentToolCall } from "../src/agents/agents-state.ts";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { toolRows } from "../src/agents/logview.tsx";
import { ActivityView } from "../src/agents/activity-view.tsx";
import type { AgentEntry } from "../src/agents/agents-state.ts";

const toolRow = (en: AgentEntry, at: number) => renderToStaticMarkup(toolRows(en, at).map((row, i) => createElement(ActivityView, { key: i, row })));

const tool: AgentToolCall = {
  name: "Bash", input: '{"command":"sleep 45"}', input_truncated: false,
  result: null, result_truncated: false,
};

function setup() {
  const page: AgentPage = {
    session: { machine_id: "m1", machine_name: "build-box", id: "s1", agent: "claude", cwd: "/srv/project", status: "working", external: false, question: null },
    entries: [], earlier: 0, plan: [], error: null, queued: 0, turns: 0, usage: null,
  };
  Object.assign(AG, { page, open: { machine_id: "m1", agent: "claude", session: "s1" } });
  AG.steps.clear();
}

test("command rows keep their compact label and expand to the actual command", () => {
  setup();
  const en = { kind: "tool", text: "Timer to recheck the build [in_progress]", tool };
  bar.innerHTML = toolRow(en, 1);
  assert.equal(bar.querySelector('.vb')?.textContent, 'Running');
  assert.equal(bar.querySelector('.tt')?.textContent, 'Timer to recheck the build');
  assert.equal(bar.querySelector('[data-at]')?.getAttribute('aria-expanded'), 'false');
  assert.equal(bar.querySelector('.call-details'), null);
  AG.steps.set(1, true);
  bar.innerHTML = toolRow(en, 1);
  assert.equal(bar.querySelector('.vb')?.textContent, 'Running');
  assert.equal(bar.querySelector('.tt')?.textContent, 'Timer to recheck the build');
  assert.ok(bar.querySelector('.call-details')?.textContent.includes('sleep 45'));
  assert.equal(bar.querySelector('[data-at]')?.getAttribute('aria-expanded'), 'true');
  assert.ok(!bar.textContent.includes('Expected wake'));
});

test("read, search, edit and Beep rows retain their original rendering", () => {
  setup();
  AG.steps.set(1, true);

  for (const [name, text] of [
    ['Read', 'Read: src/login.rs [completed]'],
    ['Grep', 'Grep: Wrong password [completed]'],
    ['Edit', 'Edit: src/login.rs [completed]'],
    ['mcp__beep__remote_run', 'Beep: Run on build-box: cargo check [completed]'],
  ]) {
    const withDetails = toolRow({ kind: 'tool', text, tool: { ...tool, name } }, 1);
    const original = toolRow({ kind: 'tool', text }, 1);
    assert.equal(withDetails, original, name);
  }
});

test("historical output and legacy command rows remain readable", () => {
  setup();
  AG.steps.set(1, true);
  bar.innerHTML = toolRow({ kind: 'tool', text: 'Bash: sleep 45', tool: { ...tool, result: '"Finished waiting"' } }, 1);
  assert.equal(bar.querySelector('.vb')?.textContent, 'Ran');
  assert.ok(bar.querySelector('.call-details')?.textContent.includes('Finished waiting'));
  bar.innerHTML = toolRow({ kind: 'tool', text: 'Bash: cargo check' }, 1);
  assert.equal(bar.querySelector('.vb')?.textContent, 'Ran');
  assert.ok(bar.querySelector('.cmd')?.textContent.includes('cargo check'));
});

test("a historical failed call with an image keeps its failure status", () => {
  setup();
  AG.steps.set(1, true);
  bar.innerHTML = toolRow({
    kind: 'tool', text: 'Bash: sleep 45 (failed)\n[image: image-0123456789abcdef]',
    tool: { ...tool, result: '"Interrupted"' },
  }, 1);
  assert.ok(bar.querySelector('.call-details')?.textContent.includes('Interrupted'));
  assert.ok(bar.querySelector('.tool')?.classList.contains('failed'));
  assert.ok(bar.querySelector('.mk')?.innerHTML.includes('Failed'));
});

test("native subagent rows keep spinning after the parent turn ends", () => {
  setup();
  AG.page!.session.status = "idle";

  for (const harness of ["codex:thread-1", "claude:parent:child"]) {
    for (const status of ["in_progress", "completed", "failed"]) {
      const en = { kind: "tool", text: `Check the fixtures [${status}]\n[steps] 1\n[session] subagent:${harness}` };
      AG.page!.entries = [en];
      bar.innerHTML = toolRow(en, 0);
      assert.equal(!!bar.querySelector('.drun'), status === "in_progress", `${harness}: ${status}`);
      assert.ok(bar.querySelector('small')?.textContent.includes(status === "in_progress" ? "working" : status === "failed" ? "failed" : "done"));
    }
  }
});

test("started agent rows spin using the child session state", () => {
  setup();
  AG.page!.session.status = "idle";
  const en = { kind: "tool", text: "Check the fixtures [completed]\n[steps] 0\n[child] child@m2 working key-1" };
  AG.page!.entries = [en];
  bar.innerHTML = toolRow(en, 0);
  assert.ok(bar.querySelector('.drun'));
  assert.ok(bar.querySelector('small')?.textContent.includes("working"));
});


test("tool times are recorded send times and absent for undated history", () => {
  setup();
  const en: AgentEntry = { kind: "tool", text: "Read src/main.rs [completed]", sent_at: 1794579242 };
  bar.innerHTML = toolRow(en, 1);
  const time = bar.querySelector("time");
  assert.equal(time?.getAttribute("datetime"), new Date(1794579242000).toISOString());
  assert.match(time?.textContent ?? "", /^\d{2}:\d{2}$/);
  assert.ok(time?.getAttribute("title"));
  bar.innerHTML = toolRow({ ...en, sent_at: null }, 1);
  assert.equal(bar.querySelector("time"), null);
});
