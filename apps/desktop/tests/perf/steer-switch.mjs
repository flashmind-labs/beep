// PLAYWRIGHT_DIR=/path/with/node_modules/ node tests/perf/steer-switch.mjs
import assert from "node:assert/strict";
import { buildSync } from "esbuild";
import { playwright, stubInit } from "./stub.mjs";

const code = buildSync({
  stdin: { contents: `export { S } from "./src/stores/bar/core.ts";
    export { AG } from "./src/parts/session/agents.ts";
    export { openAgent, stopAgentFollow } from "./src/parts/session/session.tsx";
    export { steerMessage } from "./src/parts/session/messages.ts";
    export { setPaneShown } from "./src/ui/active.ts";`, resolveDir: process.cwd() },
  bundle: true, write: false, format: "iife", globalName: "steerCheck",
}).outputFiles[0].text;
const browser = await playwright().chromium.launch({ executablePath: process.env.CHROMIUM_BIN || undefined });

try {
  const page = await browser.newPage();
  await page.setContent('<div id="bar" data-mode="window"></div>');
  await page.evaluate(stubInit, JSON.stringify({ agent_tasks: [], session_moves: [], editors: [] }));
  await page.evaluate(() => {
    const invoke = window.__TAURI__.core.invoke;
    window.__pages = Object.fromEntries(["first", "second"].map(id => [id, {
      session: { machine_id: "machine", machine_name: "build-box", id, agent: "claude", cwd: "/srv/project", status: "working", external: false, question: null },
      entries: [{ kind: "user", text: "Check the build" }], earlier: 0,
      plan: [], error: null, queued: 0, queued_messages: [], turns: 0,
    }]));
    window.__sent = 0;
    window.__TAURI__.core.invoke = async (cmd, args) => {
      if (cmd === "mesh_watch") return new Promise(() => {});
      if (cmd === "agent_view") return structuredClone(window.__pages[args.session]);

      if (cmd === "agent_steer") {
        window.__sent++;
        return new Promise(resolve => { window.__acknowledge = resolve; });
      }

      return invoke(cmd, args);
    };
  });
  await page.addScriptTag({ content: code });
  await page.evaluate(() => {
    steerCheck.setPaneShown(false);
    steerCheck.openAgent(window.__pages.first.session);
  });
  await page.waitForFunction(() => steerCheck.AG.page?.entries.length);
  await page.locator("#agmsg").fill("Use the existing fixture");
  await page.evaluate(() => steerCheck.steerMessage(document.querySelector("#agmsg")));
  assert.equal(await page.locator("#agqueue .it").count(), 1);
  await page.evaluate(() => steerCheck.openAgent(window.__pages.second.session));
  assert.equal(await page.locator("#agqueue .it").count(), 0);
  await page.evaluate(() => {
    window.__pages.first.queued = 1;
    window.__pages.first.queued_messages = ["Use the existing fixture"];
    window.__acknowledge([]);
  });
  await page.evaluate(() => steerCheck.openAgent(window.__pages.first.session));
  await page.waitForFunction(() => document.querySelector("#agqueue")?.textContent.includes("after the current step"));
  assert.equal(await page.locator("#agqueue .it").count(), 1);
  assert.equal(await page.locator("#agbody").textContent().then(text => text.includes("Use the existing fixture")), false);
  assert.equal(await page.evaluate(() => window.__sent), 1);
  assert.deepEqual(await page.evaluate(() => window.__errors), []);
  console.log("chromium: pending steers survive navigation and acknowledgments without resending");
} finally { await browser.close(); }
