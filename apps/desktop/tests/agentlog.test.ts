import test from "node:test";
import assert from "node:assert/strict";
import { answeredQuestions, imageLines, linkImagePaths, markWarnings, parseTool, subagent, tailLines } from "../src/agents/agentlog.ts";

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

test("image files a reply names become links, numbered as first named", () => {
  const { html, paths } = linkImagePaths("<p>Before <code>shots/a.png</code> and /srv/project/b.JPG, then shots/a.png again.</p><pre>x.png</pre>");
  assert.deepEqual(paths, ["shots/a.png", "/srv/project/b.JPG"]);
  assert.match(html, /<code><a class="imgl" data-img="0" data-path="shots\/a.png" title="shots\/a.png">shots\/a.png<\/a><\/code>/);
  assert.match(html, /<a class="imgl" data-img="1"[^>]*>\/srv\/project\/b.JPG<\/a>,/);
  assert.match(html, /<pre>x.png<\/pre>/, "code blocks stay as they are");
  assert.equal(linkImagePaths("no pictures here.").paths.length, 0);
});

test("a paragraph starting Warning: is marked, others are not", () => {
  assert.equal(markWarnings("<p>Warning: slow</p><p>No Warning: here</p>"), '<p class="agwarn">Warning: slow</p><p>No Warning: here</p>');
});
