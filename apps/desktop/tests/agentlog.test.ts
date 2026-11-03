import test from "node:test";
import assert from "node:assert/strict";
import { answeredQuestions, exitCode, imageLines, parseTool, subagent, tailLines, toolLines, TOOL_LINES, userHtml } from "../src/agentlog.ts";

test("image lines come out of an entry's text anywhere, in order", () => {
  const a = "image-0123456789abcdef", b = "image-fedcba9876543210";
  assert.deepEqual(imageLines(`Saved chart.png\n[image: ${a}]\nand more\n[image: ${b}]\n`), {
    text: "Saved chart.png\nand more", names: [a, b],
  });
  assert.deepEqual(imageLines(`Read: shot.png (failed)\n[image: ${a}]`), {
    text: "Read: shot.png (failed)", names: [a],
  });
  assert.deepEqual(imageLines("no pictures [image: inline] here"), {
    text: "no pictures [image: inline] here", names: [],
  });
});

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

test("a pasted block folds into a Pasted text line", () => {
  const esc = (s: string) => s.replace(/</g, "&lt;");
  const html = userHtml('look:\n<pasted_content id="295e">\na <b>\nb\n</pasted_content id="295e">\nfix it', esc);
  assert.match(html, /^look:\n<details class="paste" data-paste="0"><summary>Pasted text · 2 lines<\/summary><pre>a &lt;b>\nb<\/pre><\/details>\nfix it$/);
  assert.equal(userHtml("plain <x>", esc), "plain &lt;x>");
});

test("an opened step shows the end of its output, counting wrapped lines", () => {
  const out = Array.from({ length: 12 }, (_, i) => `line ${i}`).join("\n");
  const tail = tailLines(out);
  assert.equal(tail.shown, "line 7\nline 8\nline 9\nline 10\nline 11");
  assert.equal(tail.total, 12);
  assert.equal(tail.cut, true);
  assert.deepEqual(tailLines("a\nb"), { shown: "a\nb", total: 2, cut: false });
  // One long line of JSON is many lines on screen: only its end shows.
  const json = `{"a":"${"x".repeat(2000)}"}`;
  const long = tailLines(json);
  assert.equal(long.total, 1);
  assert.equal(long.cut, true);
  assert.ok(long.shown.length <= 500, "five wrapped lines at most");
  assert.ok(long.shown.endsWith('x"}'));
});

test("a failed command's exit code is read from its output", () => {
  assert.equal(exitCode("error: build failed\nexit code: 101"), 101);
  assert.equal(exitCode("Exited with 2"), 2);
  assert.equal(exitCode("Exit status 1"), 1);
  assert.equal(exitCode("no code here"), null);
});

test("a subagent's steps are read from its call", () => {
  assert.deepEqual(subagent("[steps] 2\n[step completed] Read a.rs\n[step in_progress] grep x\nFound it"), {
    count: 2, steps: [{ status: "completed", title: "Read a.rs" }, { status: "in_progress", title: "grep x" }], result: "Found it",
  });
  assert.equal(subagent("plain output"), null);
});

test("Codex's echo of answered questions reads as questions and answers", () => {
  const text = '<send_user_message_question_reply>[{"answer":"Use the web app text","question":"Which error text?","questionItemId":"[q1]"},{"answer":"Yes","question":"Run tests?","questionItemId":"[q2]"}]</send_user_message_question_reply>';
  assert.deepEqual(answeredQuestions(text), [{ question: "Which error text?", answer: "Use the web app text" }, { question: "Run tests?", answer: "Yes" }]);
  assert.equal(answeredQuestions("<send_user_message_question_reply>not json</send_user_message_question_reply>"), null);
  assert.equal(answeredQuestions("hello"), null);
});
