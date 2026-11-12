// Run after npm run build, with PLAYWRIGHT_DIR and optionally CHROMIUM_BIN.
import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";
import { join } from "node:path";
import { makeData } from "./data.mjs";
import { playwright, stubInit } from "./stub.mjs";

const data = makeData({ machines: 1, projects: 1, items: 0, sessions: 1 });
const session = { ...data.center_home.running[0], status: "idle", question: null };
const dist = fileURLToPath(new URL("../../dist/", import.meta.url));
const origin = "https://app.test";
const browser = await playwright().chromium.launch({ executablePath: process.env.CHROMIUM_BIN });

try {
  for (const windows of [false, true]) {
    const page = await browser.newPage({ viewport: { width: 1280, height: 900 }, userAgent: windows ? "Windows" : "Macintosh" });
    page.setDefaultTimeout(8000);
    // Intercept local assets so this test needs no listening server.
    await page.route(`${origin}/**`, async route => {
      const path = new URL(route.request().url()).pathname;

      try { await route.fulfill({ path: join(dist, path) }); }
      catch { await route.fulfill({ status: 404, body: "Missing test asset" }); }
    });
    await page.addInitScript(stubInit, JSON.stringify(data));
    await page.addInitScript(session => {
      const inner = window.__TAURI__.core.invoke;
      const entries = Array.from({ length: 320 }, (_, i) => {
        const text = i === 0 ? "needle-12" : i === 319 ? "NEEDLE-34" : `Message ${i}`;
        return { kind: i % 2 ? "agent" : "user", text, html: `<p>${text}</p>` };
      });
      window.__history = [];
      window.__TAURI__.core.invoke = async (cmd, args = {}) => {
        if (cmd === "mesh_watch") return new Promise(() => {});

        if (cmd === "agent_view") {
          const end = args.before ?? entries.length, start = Math.max(0, end - (args.last ?? 200));
          window.__history.push({ start, end });
          return { session, entries: entries.slice(start, end), earlier: start, plan: [], files: [], last_message: "", error: null, queued: 0, turns: 160 };
        }

        if (cmd === "agent_sessions") return [session];
        if (cmd === "agent_tree") return { folders: [], truncated: false };
        if (cmd === "agent_tasks" || cmd === "editors" || cmd === "project_machines") return [];
        return inner(cmd, args);
      };
    }, session);
    await page.goto(`${origin}/center.html`);
    await page.waitForFunction(() => window.__calls.center_pending > 0);
    await page.evaluate(session => { window.__pending = { agent: session }; window.__emit("center-open"); }, session);
    await page.locator("#agbody .dcoderen.agent").last().waitFor();
    await page.locator("#agmsg").focus();
    const modifier = windows ? "Control" : "Meta";
    await page.keyboard.press(`${modifier}+f`);
    const field = page.getByRole("textbox", { name: "Find in chat" });
    const regex = page.getByRole("button", { name: "Use regular expression" });
    const status = page.locator(".chat-find output");
    await field.fill("needle-[0-9]+");
    await page.waitForFunction(() => document.querySelector(".chat-find output")?.textContent === "No matches");
    assert.ok(await page.evaluate(() => window.__history.some(read => read.start === 0)), "find loads the older page");
    await regex.click();
    assert.equal(await status.textContent(), "1 of 2");
    assert.equal(await page.locator("mark.current").textContent(), "needle-12");
    const size = await page.locator(".chat-find").evaluate(el => ({ width: el.offsetWidth, height: el.offsetHeight }));
    assert.deepEqual(size, { width: 384, height: 34 });
    await page.keyboard.press("Enter");
    assert.equal(await status.textContent(), "2 of 2");
    assert.equal(await page.locator("mark.current").textContent(), "NEEDLE-34");
    await page.keyboard.press(`${modifier}+g`);
    assert.equal(await status.textContent(), "1 of 2");
    await page.keyboard.press(`${modifier}+Shift+g`);
    assert.equal(await status.textContent(), "2 of 2");
    await page.keyboard.press("Shift+Enter");
    assert.equal(await status.textContent(), "1 of 2");
    assert.ok(await page.locator("#agbody .agturn").count() < 30, "search preserves windowed rendering");
    await field.fill("[");
    await page.waitForFunction(() => document.querySelector(".chat-find output")?.textContent === "Invalid regex");
    assert.equal(await field.getAttribute("aria-invalid"), "true");
    assert.equal(await page.locator("mark.chat-match").count(), 0);
    assert.equal(await page.getByRole("button", { name: "Next match (Enter)", exact: true }).isDisabled(), true);
    await field.fill("needle");
    await page.waitForFunction(() => document.querySelector(".chat-find output")?.textContent === "1 of 2");
    assert.equal(await field.getAttribute("aria-invalid"), null);
    await page.keyboard.press(`${modifier}+f`);
    assert.equal(await page.locator(".chat-find").count(), 1);
    await page.keyboard.press("Escape");
    assert.equal(await page.locator(".chat-find").count(), 0);
    assert.equal(await page.locator("mark.chat-match").count(), 0);
    assert.equal(await page.locator("#agmsg").evaluate(el => el === document.activeElement), true);
    assert.deepEqual(await page.evaluate(() => window.__errors), []);
    console.log(`${windows ? "Windows" : "Mac"}: compact find, older history, regex, navigation, errors and focus passed`);
    await page.close();
  }
} finally { await browser.close(); }
