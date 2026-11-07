import { test } from "node:test";
import assert from "node:assert/strict";
import { childReplyHtml } from "../src/agents/childreply-state.ts";

test("the child header opens its session without making the reply body clickable", () => {
  const html = childReplyHtml({ kind: "child", id: 'k1@build-box', name: 'Run <tests>', state: "finished", text: "" }, '<p><strong>Passed</strong></p><pre><button data-copy>Copy</button><code>cargo test</code></pre>', '<svg></svg>', 'build-box · Claude Code');
  assert.match(html, /data-kid="k1@build-box"/);
  assert.match(html, /Run &lt;tests&gt;/);
  assert.match(html, /<span class="ks">Done<\/span><\/button><div class="dcoderen agent md">/);
  assert.match(html, /<strong>Passed<\/strong>/);
  assert.match(html, /data-copy/);
});

test("asking and stopped notices keep their state and escape metadata", () => {
  const part = { kind: "child" as const, id: 'k1" data-bad="yes', name: "", state: "asking", text: "" };
  const html = childReplyHtml(part, '<p>Run it?</p>', '', '<build-box>');
  assert.match(html, /Child agent/);
  assert.match(html, /Waiting on you/);
  assert.match(html, /&lt;build-box&gt;/);
  assert.doesNotMatch(html, / data-bad="yes"/);
  assert.match(childReplyHtml({ ...part, state: "stopped" }, '', '', ''), />Stopped<\/span>/);
  assert.match(childReplyHtml({ ...part, state: "closed" }, '', '', ''), />Session ended<\/span>/);
});
