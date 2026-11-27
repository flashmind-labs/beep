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
async function serveOlder(page: import("@playwright/test").Page, size = 200, tall = true) {
  await page.evaluate(({ size, tall }) => {
    const w = window as unknown as { holdOlder: boolean; releaseOlder: () => void; waiting: (() => void)[]; olderReads: number[] };
    w.holdOlder = false;
    w.waiting = [];
    w.olderReads = [];
    w.releaseOlder = () => { w.holdOlder = false; for (const go of w.waiting.splice(0)) go(); };
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = <T>(command: string, args?: Record<string, unknown>) => {
      if (command !== "agent_older") return invoke<T>(command, args);
      const before = Number(args?.before), start = Math.max(0, before - size);
      const entries = Array.from({ length: before - start }, (_, k) => {
        const i = start + k;
        return { kind: i % 2 ? "agent" : "user", text: tall ? `Message ${i}\n${"Conversation text. ".repeat(30)}` : `Message ${i}` };
      });
      w.olderReads.push(before);
      const answer = () => Promise.resolve({ earlier: start, entries } as T);
      return w.holdOlder ? new Promise<T>(done => w.waiting.push(() => done(answer() as unknown as T))).then(v => v) : answer();
    };
  }, { size, tall });
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

test("a short log sits at the bottom while older pages arrive above it", async ({ page }) => {
  const short = { ...view, entries: Array.from({ length: 20 }, (_, i) => ({ kind: i % 2 ? "agent" : "user", text: `Short ${i}` })), earlier: 15, turns: 10 };
  // Tall enough for the first three pages to fit, so the last fills it.
  await page.setViewportSize({ width: 1280, height: 1580 });
  await openDesktop(page, false, { agent_view: short });
  await serveOlder(page, 5, false);
  await page.evaluate(() => { (window as unknown as { holdOlder: boolean }).holdOlder = true; });
  await page.evaluate(agent => window.__navigation.open({ agent: { ...agent, kind: "agent" } }), session);
  const body = page.locator("#agbody");
  const reads = () => page.evaluate(() => (window as unknown as { olderReads: number[] }).olderReads.length);
  const lastBottom = () => body.evaluate(el => {
    const rows = el.querySelectorAll("[data-row]");
    return rows[rows.length - 1].getBoundingClientRect().bottom;
  });
  const fits = () => body.evaluate(el => el.scrollHeight <= el.clientHeight);

  await expect(body).toContainText("Short 19");
  await expect.poll(reads).toBe(1);
  expect(await fits()).toBe(true);
  const bottom = await lastBottom();
  expect(await body.evaluate(el => el.scrollTop)).toBe(0);

  // Two held pages arrive: the newest rows stay where they were.
  for (const wanted of [2, 3]) {
    await page.evaluate(() => { const w = window as unknown as { holdOlder: boolean; releaseOlder: () => void }; w.releaseOlder(); w.holdOlder = true; });
    await expect.poll(reads).toBe(wanted);
    expect(await body.evaluate(el => el.scrollTop)).toBe(0);
    expect(Math.abs(await lastBottom() - bottom)).toBeLessThan(1);
  }

  // The last page fills the view: the reader is at the end.
  await page.evaluate(() => { const w = window as unknown as { holdOlder: boolean; releaseOlder: () => void }; w.releaseOlder(); });
  await expect(body).toContainText("Message 0");
  await expect.poll(() => body.evaluate(el => el.scrollHeight - el.clientHeight - el.scrollTop)).toBeLessThan(2);
});

test("a complete short log sits at the top of the session body", async ({ page }) => {
  const short = { ...view, entries: [{ kind: "user", text: "First prompt" }, { kind: "agent", text: "First reply" }], earlier: 0, turns: 1 };
  await page.setViewportSize({ width: 1280, height: 1200 });
  await openDesktop(page, false, { agent_view: short });
  await page.evaluate(agent => window.__navigation.open({ agent: { ...agent, kind: "agent" } }), session);
  const body = page.locator("#agbody");

  await expect(body).toContainText("First reply");
  const box = await body.evaluate(el => {
    const bodyRect = el.getBoundingClientRect();
    const main = el.querySelector(".dagmain")!.getBoundingClientRect();
    return { topOffset: main.top - bodyRect.top, bottomSlack: bodyRect.bottom - main.bottom };
  });
  expect(box.topOffset).toBeLessThan(30);
  expect(box.bottomSlack).toBeGreaterThan(400);
});

test("a big log is placed once, before it paints, with one older read at most", async ({ page }) => {
  await openDesktop(page, false, { agent_view: view });
  await serveOlder(page);
  await page.evaluate(() => {
    const w = window as unknown as { scrolls: number };
    w.scrolls = 0;
    document.addEventListener("scroll", event => { if ((event.target as Element | null)?.id === "agbody") w.scrolls++; }, true);
  });
  await page.evaluate(agent => window.__navigation.open({ agent: { ...agent, kind: "agent" } }), session);
  const body = page.locator("#agbody");

  await expect(body).toContainText(`Message ${TOTAL - 1}`);
  await page.waitForTimeout(600);
  expect(await page.evaluate(() => (window as unknown as { scrolls: number }).scrolls)).toBeLessThanOrEqual(1);
  expect(await body.evaluate(el => el.scrollHeight - el.clientHeight - el.scrollTop)).toBeLessThan(2);
  expect(await page.evaluate(() => (window as unknown as { olderReads: number[] }).olderReads.length)).toBeLessThanOrEqual(1);
});

test("a session that asks is followed before it opens and paints at once", async ({ page }) => {
  await openDesktop(page, true);
  const asking = { ...session, id: "asking-one", title: "Needs an answer", status: "asking", question: { id: "q1", text: "Proceed?", detail: null, options: [] } };
  await page.evaluate(rows => { window.__navigation.respond("agent_sessions", rows); window.__navigation.notify("home"); }, [asking]);
  const calls = (name: string) => page.evaluate(([command, id]) => window.__navigation.calls().filter(c => c.command === command && c.args.session === id).map(c => c.args), [name, asking.id]);

  await expect.poll(async () => (await calls("agent_follow")).length).toBe(1);
  expect((await calls("agent_follow"))[0].after).toBeNull();
  expect(await calls("agent_view")).toHaveLength(0);

  await page.evaluate(() => window.__navigation.hold("transcript_kept"));
  await page.evaluate(agent => window.__navigation.open({ agent: { ...agent, kind: "agent" } }), asking);
  await expect(page.locator("#agbody")).toBeVisible();
  await expect(page.locator("#agbody .dagmain")).toBeAttached();
  expect(await calls("agent_view")).toHaveLength(0);
});

const askingRow = { ...session, id: "asking-two", title: "Needs an answer", status: "asking", question: { id: "q1", text: "Proceed?", detail: null, options: [] } };
const follows = (page: import("@playwright/test").Page, id: string) =>
  page.evaluate(wanted => window.__navigation.calls().filter(c => c.command === "agent_follow" && c.args.session === wanted).map(c => c.args), id);

test("an asking session with a kept page is followed on from the kept cursor", async ({ page }) => {
  const cursor = { start: 0, next: 2, fingerprint: "kept" };
  await openDesktop(page, true, { transcript_kept: { page: { session: askingRow, plan: [], entries: [{ kind: "user", text: "Kept question" }, { kind: "agent", text: "Kept answer" }], earlier: 0, error: null, queued: 0, queued_messages: [], turns: 1, usage: null }, cursor } });
  await page.evaluate(rows => { window.__navigation.respond("agent_sessions", rows); window.__navigation.notify("home"); }, [askingRow]);

  await expect.poll(async () => (await follows(page, askingRow.id)).length).toBe(1);
  expect((await follows(page, askingRow.id))[0].after).toEqual(cursor);
});

test("a warm follow that failed is not tried again by the next identical push", async ({ page }) => {
  await openDesktop(page, true);
  await page.evaluate(() => window.__navigation.rejectAll("agent_follow"));
  const push = () => page.evaluate(rows => { window.__navigation.respond("agent_sessions", rows); window.__navigation.notify("home"); }, [askingRow]);

  await push();
  await expect.poll(async () => (await follows(page, askingRow.id)).length).toBe(1);
  const kept = () => page.evaluate(() => window.__navigation.calls().filter(c => c.command === "transcript_kept").length);
  const reads = await kept();
  await push();
  await push();
  await page.waitForTimeout(300);
  expect((await follows(page, askingRow.id)).length).toBe(1);
  expect(await kept()).toBe(reads);
});
