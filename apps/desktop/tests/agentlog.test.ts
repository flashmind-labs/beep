import test from "node:test";
import assert from "node:assert/strict";
import { parseTool, toolLines, TOOL_LINES } from "../src/agentlog.ts";

test("a live tool entry splits into title, status and output", () => {
  assert.deepEqual(parseTool("cargo test -p beep-node [completed]\nrunning 12 tests\nok"), {
    title: "cargo test -p beep-node", status: "completed", output: "running 12 tests\nok",
  });
  assert.deepEqual(parseTool("Read file [in_progress]"), { title: "Read file", status: "in_progress", output: "" });
  assert.equal(parseTool("Write notes [failed]\nno room").status, "failed");
  assert.equal(parseTool("Fetch [pending]").status, "pending");
});

test("backticks in a title go, the words stay", () => {
  assert.equal(parseTool("Edit `src/main.rs` [completed]").title, "Edit src/main.rs");
  assert.equal(parseTool("`git status` [completed]").title, "git status");
});

test("a title over several lines keeps them, up to its status", () => {
  const t = parseTool("for f in *.rs\ndo wc -l $f; done [completed]\n12 a.rs");
  assert.equal(t.title, "for f in *.rs\ndo wc -l $f; done");
  assert.equal(t.output, "12 a.rs");
});

test("brackets in the output are not a status", () => {
  const t = parseTool("Run tests [completed]\nsee [failed] below");
  assert.equal(t.status, "completed");
  assert.equal(t.output, "see [failed] below");
});

test("output in one fenced block loses the fence", () => {
  assert.equal(parseTool("ls [completed]\n```console\na\nb\n```").output, "a\nb");
  assert.equal(parseTool("ls [completed]\n```\nx\n```\nafter").output, "```\nx\n```\nafter");
});

test("a transcript entry has a title and says only when it failed", () => {
  assert.deepEqual(parseTool("Bash: cargo test net (failed)"), { title: "Bash: cargo test net", status: "failed", output: "" });
  assert.deepEqual(parseTool("Read: /srv/project/tests/net.rs"), { title: "Read: /srv/project/tests/net.rs", status: "", output: "" });
  assert.deepEqual(parseTool(undefined), { title: "", status: "", output: "" });
});

test("long output folds to its first lines until opened", () => {
  const out = Array.from({ length: 10 }, (_, i) => `line ${i}`).join("\n");
  const closed = toolLines(out);
  assert.equal(closed.shown.split("\n").length, TOOL_LINES);
  assert.equal(closed.more, 10 - TOOL_LINES);
  assert.deepEqual(toolLines(out, true), { shown: out, more: 0 });
  assert.deepEqual(toolLines("a\nb"), { shown: "a\nb", more: 0 });
  assert.deepEqual(toolLines(""), { shown: "", more: 0 });
});
