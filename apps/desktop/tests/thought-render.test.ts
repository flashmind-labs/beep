import { bar } from "./steer-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { AG, type AgentPage } from "../src/stores/agents/agents-state.ts";
import { toggleThoughts } from "../src/parts/session/logkeys.ts";
import { drawAgentBody, turnsOf } from "../src/parts/session/logview.tsx";
import { showSessionBody } from "./session-frame-env.ts";

test("grouped live thoughts stay folded beside progress and preserve the reader's choice", () => {
  globalThis.getSelection = () => null;
  globalThis.getComputedStyle = () => ({ rowGap: "0" }) as CSSStyleDeclaration;
  const id = "thought-render";
  const page: AgentPage = {
    session: { machine_id: "m1", machine_name: "build-box", id, agent: "codex", cwd: "/srv/project", status: "working", external: false, question: null },
    entries: [{ kind: "user", text: "Check the build" },
      { kind: "thought", text: "Checking results", ms: 42000 },
      { kind: "thought", text: "Waiting for verification", ms: null }],
    earlier: 0, plan: [], error: null, queued: 0, turns: 0, usage: null,
  };
  Object.assign(AG, { open: { machine_id: "m1", agent: "codex", session: id }, page, child: null });
  showSessionBody();
  const rows = () => bar.querySelectorAll("[data-thought]");
  drawAgentBody();
  assert.equal(rows().length, 0, "thoughts start inside the collapsed progress group");
  assert.ok(bar.querySelector('[data-turn="1"]')?.textContent.includes("Thought for 42s"));
  turnsOf(id).opened.add(1);
  drawAgentBody();
  assert.equal(rows().length, 1);
  assert.equal(rows()[0].getAttribute("data-thought"), "1");
  assert.equal(rows()[0].getAttribute("aria-expanded"), "false");
  assert.ok(bar.querySelector(".dlive")?.textContent.includes("Waiting for verification"));

  turnsOf(id).opened.delete(1);
  assert.equal(toggleThoughts(), true, "the thought shortcut also opens the containing progress group");
  assert.ok(turnsOf(id).opened.has(1));
  assert.equal(rows()[0].getAttribute("aria-expanded"), "true");
  assert.ok(bar.querySelector(".kk")?.textContent.includes("Checking results"));
  assert.ok(bar.querySelector(".kk")?.textContent.includes("Waiting for verification"));

  page.entries[2].ms = 42000;
  page.entries.push({ kind: "thought", text: "Reading the final result", ms: null });
  drawAgentBody();
  assert.equal(rows().length, 1);
  assert.equal(rows()[0].getAttribute("aria-expanded"), "true");
  assert.ok(bar.querySelector(".kk")?.textContent.includes("Reading the final result"));

  page.entries[3].ms = 1000;
  page.session.status = "idle";
  drawAgentBody();
  assert.equal(rows().length, 1);
  assert.equal(rows()[0].getAttribute("aria-expanded"), "true");
  assert.ok(rows()[0].textContent.includes("Thought for 1m 25s"));

  turnsOf(id).thoughts.delete(1);
  drawAgentBody();
  assert.equal(rows()[0].getAttribute("aria-expanded"), "false");
});


test("progress groups collect alternating thoughts and tools and split at visible agent output", () => {
  globalThis.getSelection = () => null;
  globalThis.getComputedStyle = () => ({ rowGap: "0" }) as CSSStyleDeclaration;
  const id = "progress-render";
  const page: AgentPage = {
    session: { machine_id: "m1", machine_name: "build-box", id, agent: "codex", cwd: "/srv/project", status: "working", external: false, question: null },
    entries: [{ kind: "user", text: "Check the build" },
      { kind: "thought", text: "Check version", ms: 8000 },
      { kind: "tool", text: "Read version.txt [completed]" },
      { kind: "thought", text: "Check tests", ms: 6000 },
      { kind: "tool", text: "cargo test [completed]" },
      { kind: "agent", text: "The version matches. Checking the build next." },
      { kind: "thought", text: "Build it", ms: 2000 },
      { kind: "tool", text: "cargo build [in_progress]" }],
    earlier: 0, plan: [], error: null, queued: 0, turns: 0, usage: null,
  };
  Object.assign(AG, { open: { machine_id: "m1", agent: "codex", session: id }, page, child: null });
  AG.steps.clear();
  showSessionBody();
  drawAgentBody();
  const heads = () => bar.querySelectorAll(".hd[data-turn]");
  assert.equal(heads().length, 2);
  assert.equal(heads()[0].getAttribute("data-turn"), "1");
  assert.equal(heads()[1].getAttribute("data-turn"), "6");
  assert.ok(heads()[0].textContent.includes("Thought for 14s"));
  assert.equal(bar.querySelectorAll("[data-thought]").length, 0);
  assert.equal(bar.querySelectorAll<HTMLElement>(".tool.row").length, 1);
  assert.ok(bar.querySelector('.tool.row[data-row="7"]')?.textContent.includes("cargo build"));
  assert.ok(bar.textContent.includes("The version matches."));

  turnsOf(id).opened.add(1);
  drawAgentBody();
  assert.equal(bar.querySelectorAll("[data-thought]").length, 2);
  assert.equal(bar.querySelectorAll<HTMLElement>(".tool.row").length, 3);
  assert.equal(heads()[1].getAttribute("aria-expanded"), "false");

  page.entries[7].text = "cargo build [completed]";
  page.entries.push({ kind: "thought", text: "Inspect result", ms: 1000 });
  drawAgentBody();
  assert.equal(heads().length, 2, "new activity accumulates in the existing second group");
  assert.equal(heads()[0].getAttribute("aria-expanded"), "true");
  assert.equal(heads()[1].getAttribute("data-turn"), "6");
  page.entries.push({ kind: "agent", text: "Build passed." });
  page.session.status = "idle";
  drawAgentBody();
  assert.equal(heads().length, 2);
  assert.ok(bar.textContent.includes("Build passed."));
  assert.ok(bar.textContent.includes("The version matches."));
});

test("the latest call stays visible until completion, while manual expansion survives it", () => {
  globalThis.getSelection = () => null;
  globalThis.getComputedStyle = () => ({ rowGap: "0" }) as CSSStyleDeclaration;
  const id = "live-command-preview";
  const page: AgentPage = {
    session: { machine_id: "m1", machine_name: "build-box", id, agent: "codex", cwd: "/srv/project", status: "working", external: false, question: null },
    entries: [{ kind: "user", text: "Check the build" },
      { kind: "tool", text: "cargo check [completed]" },
      { kind: "tool", text: "cargo test [in_progress]" }],
    earlier: 40, plan: [], error: null, queued: 0, turns: 0, usage: null,
  };
  Object.assign(AG, { open: { machine_id: "m1", agent: "codex", session: id }, page, child: null });
  AG.steps.clear();
  showSessionBody();
  const rows = () => bar.querySelectorAll<HTMLElement>(".tool.row");
  const head = () => bar.querySelector('.hd[data-turn="41"]')!;
  drawAgentBody();
  assert.equal(rows().length, 1);
  assert.equal(rows()[0].dataset.row, "42");
  assert.equal(rows()[0].getAttribute("aria-expanded"), "false", "output stays compact");
  assert.ok(rows()[0].textContent.includes("Running"));
  assert.ok(head().textContent.includes("Ran 2 commands"), "summary includes the preview");
  assert.equal(head().getAttribute("aria-expanded"), "false");

  page.entries[2].text = "cargo test [completed]";
  page.entries.push({ kind: "thought", text: "Checking the results", ms: null });
  drawAgentBody();
  assert.equal(rows().length, 1, "a thought does not hide the latest call");
  assert.equal(rows()[0].dataset.row, "42");

  page.entries.push({ kind: "agent", text: "Tests passed. Checking the build." },
    { kind: "tool", text: "cargo build [in_progress]" });
  drawAgentBody();
  assert.equal(rows().length, 1, "the new call replaces the old preview across progress groups");
  assert.equal(rows()[0].dataset.row, "45");
  turnsOf(id).opened.add(41);
  drawAgentBody();
  assert.equal(bar.querySelectorAll("[data-collapse]").length, 1);
  const collapse = bar.querySelector("[data-collapse]")!;
  assert.equal(collapse.getAttribute("aria-label"), "Collapse activity");
  assert.ok(collapse.querySelector("svg"));
  assert.equal(collapse.textContent, "");

  page.entries[5].text = "cargo build [completed]";
  page.entries.push({ kind: "agent", text: "Build passed." });
  page.session.status = "idle";
  drawAgentBody();
  assert.equal(head().getAttribute("aria-expanded"), "true", "manual expansion survives completion");
  assert.equal(bar.querySelector('.tool.row[data-row="45"]'), null, "the automatic preview folds on completion");
  assert.ok(bar.textContent.includes("Build passed."));

  turnsOf(id).opened.delete(41);
  AG.steps.set(42, true);
  drawAgentBody();
  assert.equal(rows().length, 1, "an explicitly opened call remains visible after completion");
  assert.equal(rows()[0].dataset.row, "42");
  AG.steps.set(42, false);
  drawAgentBody();
  assert.equal(rows().length, 0);

  page.entries[5].text = "cargo build [failed]\nexit 1";
  drawAgentBody();
  assert.equal(rows().length, 1, "a failed final call remains visible");
  assert.equal(rows()[0].dataset.row, "45");
});

test("streaming waits while a reply control is focused, then updates the same reply", () => {
  globalThis.getSelection = () => null;
  globalThis.getComputedStyle = () => ({ rowGap: "0" }) as CSSStyleDeclaration;
  const id = "reply-focus";
  const page: AgentPage = {
    session: { machine_id: "m1", machine_name: "build-box", id, agent: "codex", cwd: "/srv/project", status: "working", external: false, question: null },
    entries: [{ kind: "user", text: "Question" }, { kind: "agent", text: "First", html: "<p>First</p><pre><code>one</code></pre>" }],
    earlier: 0, plan: [], error: null, queued: 0, turns: 0, usage: null,
  };
  Object.assign(AG, { open: { machine_id: "m1", agent: "codex", session: id }, page, child: null });
  showSessionBody();
  drawAgentBody();
  const reply = bar.querySelector<HTMLElement>('.dcoderen.agent[data-row="1"]');
  const copy = reply?.querySelector<HTMLButtonElement>("[data-copy]");
  assert.ok(reply && copy);
  copy.focus();
  page.entries[1].html = "<p>Updated</p><pre><code>one</code></pre>";
  drawAgentBody(undefined, true);
  assert.equal(document.activeElement, copy);
  assert.ok(reply.textContent?.includes("First"));
  copy.blur();
  drawAgentBody(undefined, true);
  assert.equal(bar.querySelector('.dcoderen.agent[data-row="1"]'), reply);
  assert.ok(reply.textContent?.includes("Updated"));
});

test("a log the node writes again from the start does not take the old log's turn times", () => {
  globalThis.getSelection = () => null;
  globalThis.getComputedStyle = () => ({ rowGap: "0" }) as CSSStyleDeclaration;
  const id = "rewritten-log";
  const session = { machine_id: "m1", machine_name: "build-box", id, agent: "claude", cwd: "/srv/project", status: "idle", external: false, question: null };
  const page = (entries: AgentPage["entries"], status: string): AgentPage =>
    ({ session: { ...session, status }, entries, earlier: 0, plan: [], error: null, queued: 0, turns: 0, usage: null });
  showSessionBody();
  const since = () => Number(bar.querySelector(".dlive [data-since]")?.getAttribute("data-since"));

  Object.assign(AG, { open: { machine_id: "m1", agent: "claude", session: id }, page: page([{ kind: "user", text: "Add accounts" }, { kind: "agent", text: "Done" }], "idle"), child: null });
  drawAgentBody();
  AG.page = page([{ kind: "user", text: "Add accounts" }, { kind: "agent", text: "Done" }, { kind: "user", text: "Show the UI" }], "working");
  drawAgentBody();
  turnsOf(id).starts.set(2, Date.now() - 12 * 3600_000);

  // The node restarted and resumed the session in a new log.
  AG.page = page([{ kind: "other", text: "Resumed after beep-node restarted" }, { kind: "agent", text: "Here it is" }, { kind: "user", text: "Improve the UI" }], "working");
  drawAgentBody();
  assert.ok(!(Date.now() - since() > 3600_000), "the new turn shows no start from the old log");

  AG.page = page([...AG.page.entries, { kind: "agent", text: "Done" }, { kind: "user", text: "And the tests" }], "working");
  drawAgentBody();
  assert.ok(Date.now() - since() < 60_000, "a turn after it is timed from when it came");
});
