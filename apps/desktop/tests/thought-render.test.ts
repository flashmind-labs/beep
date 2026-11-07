import { bar } from "./dom-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { AG, type AgentPage } from "../src/agents/agents.ts";
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
  assert.equal(rows().length, 1);
  assert.equal(rows()[0].getAttribute("data-thought"), "1");
  assert.equal(rows()[0].getAttribute("aria-expanded"), "false");
  assert.ok(bar.querySelector(".dlive")?.textContent.includes("Waiting for verification"));

  turnsOf(id).thoughts.add(1);
  drawAgentBody();
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
