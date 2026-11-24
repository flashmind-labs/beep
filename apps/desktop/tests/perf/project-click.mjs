// A sync update between mouse down and mouse up must not swallow a session click.
// PLAYWRIGHT_DIR=/path/with/node_modules/ node tests/perf/project-click.mjs [webkit|chromium]
import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";
import { makeData } from "./data.mjs";
import { playwright, serve, stubInit } from "./stub.mjs";

const data = makeData({ machines: 1, projects: 2, items: 0, sessions: 2 });
const project = data.center_home.projects[0];
const session = data.center_home.running[0];
const past = { ...session, id: "past-session", title: "Past session", status: "ended" };
const { server, origin } = await serve(fileURLToPath(new URL("../../dist", import.meta.url)));
const kind = process.argv[2] || "webkit";
const browser = await playwright()[kind].launch({ executablePath: kind === "webkit" ? process.env.WEBKIT_BIN : process.env.CHROMIUM_BIN });

try {
  const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
  page.setDefaultTimeout(8000);
  await page.addInitScript(stubInit, JSON.stringify(data));
  await page.addInitScript(({ project, session, past }) => {
    const inner = window.__TAURI__.core.invoke;
    window.__revision = 0;
    window.__TAURI__.core.invoke = async (cmd, args = {}) => {
      if (cmd === "mesh_watch") {
        window.__sync = () => args.onEvent.onmessage({ machine_id: session.machine_id, topic: "sync.*", revision: ++window.__revision });
        return new Promise(() => {});
      }

      if (cmd === "project_machines") return [{ id: session.machine_id, name: "build-box", online: true, folder: session.cwd, checking: false, checked_at: 0 }];
      if (cmd === "project_syncthing" || cmd === "project_git_state" || cmd === "editors") return [];
      if (cmd === "project_sessions") return [session, past];
      if (cmd === "project_sync_detail") return { project: project.name, pinned: false, paused: false, machines: [{
        machine_id: session.machine_id, machine_name: "build-box", folder: session.cwd,
        status: window.__revision % 2 ? "scanning" : "in_sync", files: 20, bytes: 100,
        pending: 0, conflicts: [], error: null, hint: null, paused: false,
        progress: null, waiting: [], can_resolve: false,
      }] };
      if (cmd === "project_sync") return [{ project: project.name, machine_id: session.machine_id, status: window.__revision % 2 ? "scanning" : "in_sync", files: 20, pending: 0, error: null }];
      if (cmd === "agent_view") return { session: args.session === past.id ? past : session, plan: [], error: null, queued: 0, turns: 0, entries: [], earlier: 0 };
      if (cmd === "agent_sessions") return [session, past];
      if (cmd === "agent_tasks") return [];
      return inner(cmd, args);
    };
  }, { project, session, past });
  await page.goto(`${origin}/center.html`);
  await page.waitForFunction(() => !!window.__sync);
  const open = tile => page.evaluate(({ project, tile }) => {
    window.__pending = { projects: { project, tile } };
    window.__emit("center-open");
  }, { project: project.name, tile });

  for (const [tile, selector, title] of [
    ["running", "#sec-project-agents .cc-row", session.title],
    ["agents", ".pj-det .pv-row", past.title],
  ]) {
    await open(tile);
    const row = page.locator(selector).filter({ hasText: title });
    await row.waitFor();
    await row.evaluate(el => {
      window.__pressed = el;
      window.__detached = false;
      window.__observer = new MutationObserver(changes => {
        if (changes.some(c => [...c.removedNodes].some(n => n === el || n.contains(el)))) window.__detached = true;
      });
      window.__observer.observe(document.body, { childList: true, subtree: true });
    });
    const button = tile === "running" ? row.locator("button.t") : row;
    const box = await button.boundingBox();
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    await page.mouse.down();
    // Await an observable navigation change, so the refresh has actually drawn.
    const before = await page.locator('.pj-li[title="Machines"]').textContent();
    await page.evaluate(() => window.__sync());
    await page.waitForFunction(before => document.querySelector('.pj-li[title="Machines"]')?.textContent !== before, before);
    await page.mouse.up();
    await page.waitForFunction(title => document.getElementById("agtitle")?.textContent === title, title);
    assert.equal(await page.evaluate(() => window.__pressed.isConnected), true);
    assert.equal(await page.evaluate(() => { window.__observer.disconnect(); return window.__detached; }), false);
    await page.keyboard.press("Escape");
    await row.waitFor();
  }

  assert.deepEqual(await page.evaluate(() => window.__errors.filter(e => e !== "ResizeObserver loop completed with undelivered notifications.")), []);
  console.log(`Passed (${kind}): running and past sessions open on the first click across sync updates.`);
} finally {
  await browser.close();
  server.close();
}
