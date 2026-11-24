// Browser regression for opening native subagents through the regular session UI.
// PLAYWRIGHT_DIR=/path/with/node_modules/ CHROMIUM_BIN=/path/to/chromium node tests/perf/native-session.mjs
import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";
import { makeData } from "./data.mjs";
import { playwright, serve, stubInit } from "./stub.mjs";

const parent = { machine_id: "fake-machine-00", machine_name: "build-box", id: "parent-session", agent: "codex", title: "Check the login test", cwd: "/srv/project", status: "working", external: false, started_at: 0, last_activity: 0 };
const child = { ...parent, id: "subagent:codex:thread-1", title: "Review the fixture", status: "ended", external: true };
const parentEntries = [
  { kind: "user", text: "Check the login test" },
  { kind: "tool", text: "Review the fixture [completed]\n[steps] 1\n[agent] Explorer\n[session] subagent:codex:thread-1\nThe fixture is stale." },
];
const childEntries = [
  { kind: "user", text: "Compare the fixture against the current message." },
  { kind: "thought", text: "I will compare the assertion with the implementation.", html: "<p>I will compare the assertion with the implementation.</p>", ms: 1400 },
  { kind: "tool", text: "cargo test [completed]\nThe login fixture passed." },
  { kind: "agent", text: "The fixture is stale.", html: "<p>The fixture is stale.</p>" },
];
const { server, origin } = await serve(fileURLToPath(new URL("../../dist", import.meta.url)));
const browser = await playwright().chromium.launch({ executablePath: process.env.CHROMIUM_BIN });

try {
  const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
  page.setDefaultTimeout(8000);
  page.on("pageerror", error => console.error(error.stack));
  await page.addInitScript(stubInit, JSON.stringify(makeData()));
  await page.addInitScript(({ parent, child, parentEntries, childEntries }) => {
    const inner = window.__TAURI__.core.invoke;
    window.__sessionReads = [];
    window.__TAURI__.core.invoke = async (cmd, args = {}) => {
      if (cmd === "agent_view") {
        window.__sessionReads.push(args.session);
        return { session: args.session === child.id ? child : parent, plan: [], error: null, queued: 0, turns: 0, entries: args.session === child.id ? childEntries : parentEntries, earlier: 0 };
      }

      if (cmd === "agent_sessions") return [parent];
      if (cmd === "editors") return [];
      if (cmd === "agent_tree") return { folders: [{ path: "", more: 0, entries: [{ name: "tests", folder: true, size: 0, ignored: false, change: null }] }], truncated: false };
      if (cmd === "agent_modes") return null;
      if (cmd === "agent_tasks") return [];
      return inner(cmd, args);
    };
  }, { parent, child, parentEntries, childEntries });
  await page.goto(`${origin}/center.html`);
  await page.waitForSelector("#side button");
  await page.evaluate(parent => { window.__pending = { agent: parent }; window.__emit("center-open"); }, parent);
  await page.locator("#agbody [data-native]").click();
  await page.waitForFunction(title => document.getElementById("agtitle")?.textContent === title, child.title);
  await page.waitForFunction(() => document.getElementById("agbody")?.textContent.includes("Compare the fixture against the current message."));
  assert.ok(await page.locator("#agbody .agturn").count());
  assert.equal(await page.locator("#agbody .dcrumb").count(), 1);
  assert.equal(await page.locator("#agnativemsg").isDisabled(), true);
  assert.ok(await page.evaluate(id => window.__sessionReads.includes(id), child.id));
  await page.locator("#agbody .hd[data-turn]").first().click();
  await page.locator("#agbody .dcoderen.tool.row").first().click();
  await page.waitForFunction(() => document.getElementById("agbody")?.textContent.includes("The login fixture passed."));
  if (process.env.SCREENSHOT) await page.screenshot({ path: process.env.SCREENSHOT });
  await page.locator("#agup").click();
  await page.waitForFunction(title => document.getElementById("agtitle")?.textContent === title, parent.title);
  await page.locator("#agbody [data-native]").click();
  await page.waitForFunction(title => document.getElementById("agtitle")?.textContent === title, child.title);
  await page.keyboard.press("Escape");
  await page.waitForFunction(title => document.getElementById("agtitle")?.textContent === title, parent.title);
  const errors = await page.evaluate(() => window.__errors);
  assert.deepEqual(errors, []);
  console.log("Passed: native child uses the normal transcript and returns to its parent by breadcrumb and Escape.");
} finally {
  await browser.close();
  server.close();
}
