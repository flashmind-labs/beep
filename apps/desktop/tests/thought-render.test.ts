import { bar } from "./dom-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { AG, type AgentPage } from "../src/agents/agents.ts";
import { toggleThoughts } from "../src/agents/logkeys.ts";
import * as agentlog from "../src/agents/agentlog.ts";
import { drawAgentBody, turnsOf } from "../src/agents/logview.ts";

test("grouped live thoughts stay folded beside progress and preserve the reader's choice", () => {
  globalThis.getSelection = () => null;
  globalThis.getComputedStyle = () => ({ rowGap: "0" }) as CSSStyleDeclaration;
  const id = "thought-render";
  const page: AgentPage = {
    session: { machine_id: "m1", machine_name: "build-box", id, agent: "codex", cwd: "/srv/project", status: "working", external: false, question: null },
    entries: [{ kind: "user", text: "Check the build" },
      { kind: "thought", text: "Checking results", ms: 42000 },
      { kind: "thought", text: "Waiting for verification", ms: null }],
    earlier: 0, plan: [], files: [], last_message: "", error: null, queued: 0, turns: 0,
  };
  Object.assign(AG, { open: { machine_id: "m1", agent: "codex", session: id }, page, log: agentlog, child: null });
  bar.innerHTML = '<div id="agbody"></div>';
  Object.assign(bar.querySelector("#agbody")!, { clientHeight: 800 });
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
    earlier: 0, plan: [], files: [], last_message: "", error: null, queued: 0, turns: 0,
  };
  Object.assign(AG, { open: { machine_id: "m1", agent: "codex", session: id }, page, log: agentlog, child: null });
  AG.steps.clear();
  bar.innerHTML = '<div id="agbody"></div>';
  Object.assign(bar.querySelector("#agbody")!, { clientHeight: 800 });
  drawAgentBody();
  const heads = () => bar.querySelectorAll(".hd[data-turn]");
  assert.equal(heads().length, 2);
  assert.equal(heads()[0].getAttribute("data-turn"), "1");
  assert.equal(heads()[1].getAttribute("data-turn"), "6");
  assert.ok(heads()[0].textContent.includes("Thought for 14s"));
  assert.equal(bar.querySelectorAll("[data-thought]").length, 0);
  assert.equal(bar.querySelectorAll(".tool.row").length, 0);
  assert.ok(bar.textContent.includes("The version matches."));

  turnsOf(id).opened.add(1);
  drawAgentBody();
  assert.equal(bar.querySelectorAll("[data-thought]").length, 2);
  assert.equal(bar.querySelectorAll(".tool.row").length, 2);
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
