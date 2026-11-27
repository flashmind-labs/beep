import test from "node:test";
import assert from "node:assert/strict";
import { answeredQuestions, exitCode, imageLines, linkCodeFiles, linkCodeSessions, linkImagePaths, markWarnings, parseTool, sentFiles, subagent, tailLines } from "../src/stores/agents/agentlog.ts";
import { findLinkedSession, linkedSession, parseSessionRef, sessionHref } from "../src/stores/agents/nesting-state.ts";

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
    count: 2, steps: [{ status: "completed", title: "Read a.rs" }, { status: "in_progress", title: "grep x" }], result: "Found it", model: null,
  });
  assert.equal(subagent("plain output"), null);
});

test("start calls and child cards preserve literal multiline prompts", () => {
  const prompt = "Fix `login`.\n\nKeep <session> and `cargo test` intact.\n" + "Last instruction. ".repeat(200).trimEnd();
  const start = parseTool(`Beep: Start codex agent: ${prompt} [completed]\n{} `);
  assert.equal(start.title, `Beep: Start codex agent: ${prompt}`);
  const card = parseTool(`Login fix\n${prompt} [in_progress]\n[steps] 0\n[agent] codex on build-box\n[child] s1@build-box working key`);
  assert.equal(card.title, `Login fix\n${prompt}`);
  assert.equal(subagent(card.output)?.child?.id, "s1@build-box");
  assert.equal(subagent(card.output)?.result, "");
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

test("inline code naming a file becomes a file reference", () => {
  assert.equal(
    linkCodeFiles("<p>See <code>src/a.rs:12</code>, <code>self.mcp</code>.</p><pre><code>src/b.rs</code></pre>"),
    '<p>See <code class="fref" data-file="src/a.rs:12" role="link" tabindex="0">src/a.rs:12</code>, <code>self.mcp</code>.</p><pre><code>src/b.rs</code></pre>',
  );
});

test("Claude API errors use the warning color without changing other paragraphs", () => {
  const error = 'API Error: 529 Overloaded. Check <a href="https://status.claude.com">status</a>.';
  assert.equal(markWarnings(`<p>${error}</p>`), `<p class="agwarn">${error}</p>`);
  assert.equal(markWarnings("<p>API Error: 401 Unauthorized</p>"), '<p class="agwarn">API Error: 401 Unauthorized</p>');
  const quota = "\u26a0 Individual quota reached. Resets in 2h26m38s.";
  assert.equal(markWarnings(`<p>${quota}</p>`), `<p class="agwarn">${quota}</p>`);
  const ordinary = "<p>Investigating API Error: 529</p><pre><code>API Error: example</code></pre><p>Done.</p>";
  assert.equal(markWarnings(ordinary), ordinary);
});

test("subagent models preserve native links and result text", () => {
  assert.deepEqual(subagent("[steps] 0\n[agent] general-purpose\n[session] subagent:claude:parent:child\n[model] sonnet\nDone"), {
    count: 0, steps: [], agent: "general-purpose", model: "sonnet", session: "subagent:claude:parent:child", result: "Done",
  });
  assert.equal(subagent("[steps] 0\n[model] gpt-example\nDone")?.model, "gpt-example");
});

test("exitCode reads every spelling and takes the last by default", () => {
  assert.equal(exitCode("exited with code 2"), 2);
  assert.equal(exitCode("Process exited with status 3"), 3);
  assert.equal(exitCode("status 1"), 1);
  assert.equal(exitCode("exit code: 101"), 101);
  assert.equal(exitCode("exit 101"), 101);
  assert.equal(exitCode("exit code 1\nrun exit 0 to quit"), 0);
  assert.equal(exitCode("exit code 1\nrun exit 0 to quit", true), 1);
  assert.equal(exitCode("nothing"), null);
});

test("a running file send names the bytes it has sent, apart from its output", () => {
  assert.deepEqual(sentFiles("[sent] 412000000 1200000000"), { sent: "412 MB of 1.2 GB", output: "" });
  assert.deepEqual(sentFiles("[sent] 0 2048\n- a.apk: sent"), { sent: "0 B of 2.0 KB", output: "- a.apk: sent" });
  assert.deepEqual(sentFiles("- a.apk: sent"), { sent: null, output: "- a.apk: sent" });
});

test("session link URLs and inline session references parse and format cleanly", () => {
  assert.deepEqual(linkedSession("beep://session/session_abc123"), { id: "session_abc123", machine: null });
  assert.deepEqual(linkedSession("beep://session/00000000-0000-4000-8000-000000000042@build-box"), {
    id: "00000000-0000-4000-8000-000000000042", machine: "build-box",
  });
  assert.deepEqual(linkedSession("BEEP://SESSION/s1@gpu-box/"), { id: "s1", machine: "gpu-box" });
  assert.equal(linkedSession("beep://session/"), null);
  assert.equal(linkedSession("https://example.com/session/s1"), null);
  assert.equal(parseSessionRef("bad id"), null);
  assert.equal(sessionHref({ id: "s1", machine_name: "Build Box" }), "beep://session/s1@build-box");
  assert.equal(sessionHref({ id: "s1", machine_name: "" }), "beep://session/s1");

  const rows = [
    { id: "s1", machine_id: "m1", machine_name: "Build Box", title: "Migration work" },
    { id: "s1", machine_id: "m2", machine_name: "GPU Box", title: "Other" },
    { id: "s2", machine_id: "m2", machine_name: "GPU Box", title: null },
  ];
  assert.equal(findLinkedSession(rows, { id: "s1", machine: "gpu-box" })?.machine_id, "m2");
  assert.equal(findLinkedSession(rows, { id: "s1", machine: null }, "m2")?.machine_id, "m2");
  assert.equal(findLinkedSession(rows, { id: "s1", machine: "m1" })?.machine_id, "m1");
  assert.equal(findLinkedSession(rows, { id: "s3", machine: null }), undefined);
});

test("inline code matching another known session becomes a session reference", () => {
  const rows = [
    { id: "current", machine_id: "m1", machine_name: "Build Box", title: "Here" },
    { id: "session_abc123", machine_id: "m1", machine_name: "Build Box", title: "Example work" },
    { id: "00000000-0000-4000-8000-000000000042", machine_id: "m2", machine_name: "GPU Box", title: null },
  ];
  const html = "<p>See <code>session_abc123</code>, <code>00000000-0000-4000-8000-000000000042@gpu-box</code>, <code>current</code>, and <code>unknown</code>. <a href=\"beep://session/session_abc123\"><code>session_abc123</code></a></p><pre><code>session_abc123</code></pre>";
  const out = linkCodeSessions(html, rows, { machine_id: "m1", id: "current" });
  assert.equal(
    out,
    '<p>See <code class="fref" data-session="session_abc123" title="Open Example work" role="link" tabindex="0">session_abc123</code>, <code class="fref" data-session="00000000-0000-4000-8000-000000000042@gpu-box" title="Open session" role="link" tabindex="0">00000000-0000-4000-8000-000000000042@gpu-box</code>, <code>current</code>, and <code>unknown</code>. <a href="beep://session/session_abc123"><code>session_abc123</code></a></p><pre><code>session_abc123</code></pre>',
  );
});
