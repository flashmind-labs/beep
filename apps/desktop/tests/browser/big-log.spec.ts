import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const session = {
  machine_id: "build-box", machine_name: "build-box", id: "big-session", agent: "codex",
  title: "Big conversation", cwd: "/srv/project", status: "idle", external: false,
  doing: null, question: null, started_at: 1, last_activity: 1,
};
const TOTAL = 6000;
const NEWEST = 65;
const entry = (i: number) => ({ kind: i % 2 ? "agent" : "user", text: `Message ${i}\n${"Conversation text. ".repeat(30)}` });
const view = {
  session, model: null, plan: [], entries: Array.from({ length: NEWEST }, (_, i) => entry(TOTAL - NEWEST + i)),
  earlier: TOTAL - NEWEST, error: null, queued: 0, queued_messages: [], turns: 3000,
};

/// Answers `agent_older` with the entries before `before`, a page at a time. While
/// `window.holdOlder` is set, an answer waits for `window.releaseOlder()`.
async function serveOlder(page: import("@playwright/test").Page) {
  await page.evaluate(() => {
    const w = window as unknown as { holdOlder: boolean; releaseOlder: () => void; waiting: (() => void)[]; olderReads: number[] };
    w.holdOlder = false;
    w.waiting = [];
    w.olderReads = [];
    w.releaseOlder = () => { w.holdOlder = false; for (const go of w.waiting.splice(0)) go(); };
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = <T>(command: string, args?: Record<string, unknown>) => {
      if (command !== "agent_older") return invoke<T>(command, args);
      const before = Number(args?.before), start = Math.max(0, before - 200);
      const entries = Array.from({ length: before - start }, (_, k) => {
        const i = start + k;
        return { kind: i % 2 ? "agent" : "user", text: `Message ${i}\n${"Conversation text. ".repeat(30)}` };
      });
      w.olderReads.push(before);
      const answer = () => Promise.resolve({ earlier: start, entries } as T);
      return w.holdOlder ? new Promise<T>(done => w.waiting.push(() => done(answer() as unknown as T))).then(v => v) : answer();
    };
  });
}

test("a big log opens at its latest message with a bounded read, and reads on near the top", async ({ page }) => {
  await openDesktop(page, false, { agent_view: view });
  await serveOlder(page);
  await page.evaluate(agent => window.__navigation.open({ agent: { ...agent, kind: "agent" } }), session);
  const body = page.locator("#agbody");
  const reads = () => page.evaluate(() => (window as unknown as { olderReads: number[] }).olderReads.length);
  const gap = () => body.evaluate(el => el.scrollHeight - el.clientHeight - el.scrollTop);

  await expect(body).toContainText(`Message ${TOTAL - 1}`);
  await page.waitForTimeout(600);
  const opened = await reads();
  expect(opened).toBeLessThanOrEqual(3);
  expect(await gap()).toBeLessThan(2);

  await body.evaluate(el => { el.scrollTop = 0; });
  await expect.poll(reads).toBeGreaterThan(opened);
});

test("hiding the window while an older page arrives keeps the row in view", async ({ page }) => {
  await openDesktop(page, false, { agent_view: view });
  await serveOlder(page);
  await page.evaluate(agent => window.__navigation.open({ agent: { ...agent, kind: "agent" } }), session);
  const body = page.locator("#agbody");
  const reads = () => page.evaluate(() => (window as unknown as { olderReads: number[] }).olderReads.length);
  const firstRow = () => body.evaluate(el => {
    const top = el.getBoundingClientRect().top, walk = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
    for (let n = walk.nextNode(); n; n = walk.nextNode()) {
      const found = /Message \d+/.exec(n.textContent ?? "");
      const at = n.parentElement?.getBoundingClientRect();
      if (found && at && at.height > 0 && at.bottom > top + 1) return found[0];
    }
    return null;
  });

  await expect(body).toContainText(`Message ${TOTAL - 1}`);
  await page.waitForTimeout(600);
  const settled = await reads();

  // The reader goes up near the top, so the next page is read; it is held while the window hides.
  await page.evaluate(() => { (window as unknown as { holdOlder: boolean }).holdOlder = true; });
  await body.evaluate(el => { el.scrollTop = 400; });
  await expect.poll(reads).toBeGreaterThan(settled);
  await page.waitForTimeout(200);
  const row = await firstRow();
  expect(row).not.toBeNull();

  await page.evaluate(() => window.__navigation.emit("center-hidden", null));
  await page.evaluate(() => (window as unknown as { releaseOlder: () => void }).releaseOlder());
  await page.waitForTimeout(500);
  await page.evaluate(() => window.__navigation.emit("center-shown", null));
  await page.waitForTimeout(300);

  expect(await firstRow()).toBe(row);
});

test("a row that grows during a wheel glide does not release the end on a nudge down", async ({ page }) => {
  await openDesktop(page, false, { agent_view: view });
  await serveOlder(page);
  await page.evaluate(agent => window.__navigation.open({ agent: { ...agent, kind: "agent" } }), session);
  const body = page.locator("#agbody");
  const state = () => body.evaluate((el: HTMLElement & { stuck?: boolean }) => ({ stuck: el.stuck, gap: el.scrollHeight - el.clientHeight - el.scrollTop }));

  await expect(body).toContainText(`Message ${TOTAL - 1}`);
  await page.waitForTimeout(600);
  expect((await state()).stuck).toBe(true);

  // The wheel's glide holds the pin back while the last row grows.
  await body.evaluate(el => {
    el.dispatchEvent(new WheelEvent("wheel", { deltaY: 1, bubbles: true }));
    const grown = document.createElement("div");
    grown.style.height = "120px";
    el.querySelectorAll("[data-row]").item(el.querySelectorAll("[data-row]").length - 1).appendChild(grown);
  });
  await page.evaluate(() => new Promise(done => requestAnimationFrame(() => requestAnimationFrame(done))));
  await body.evaluate(el => { el.scrollTop += 5; });
  await page.waitForTimeout(300);

  expect((await state()).stuck).toBe(true);
});
