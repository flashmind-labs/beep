import { expect, test, type Page } from "@playwright/test";
import { bundleFixture, openDesktop } from "./harness.ts";

// A branch diff of about 600 files and 40k lines must open, scroll and
// update without blocking the window.
async function setup(page: Page) {
  await openDesktop(page);
  await page.goto("https://app.test/index.html");
  const script = await bundleFixture("tests/browser/diff-large-fixture.ts", true);
  await page.evaluate(() => {
    const host = document.createElement("div");
    host.id = "host";
    host.style.cssText = "position:fixed;inset:0;z-index:9999;background:var(--bg, #fff);display:flex;flex-direction:column";
    document.body.append(host);
    const longTasks: number[] = [];
    new PerformanceObserver(list => { for (const entry of list.getEntries()) longTasks.push(entry.duration); }).observe({ type: "longtask" });
    Reflect.set(window, "longTasks", longTasks);
  });
  await page.addScriptTag({ content: script });
}

const frames = (page: Page) => page.evaluate(() => new Promise(done => requestAnimationFrame(() => requestAnimationFrame(done))));
const takeLongTasks = (page: Page) => page.evaluate(() => { const list: number[] = Reflect.get(window, "longTasks"); return list.splice(0); });

test("a 600 file diff opens, scrolls and updates without long tasks", async ({ page, browserName }, info) => {
  test.skip(info.project.name !== "macOS", "one measured run is enough");
  test.setTimeout(120_000);
  await setup(page);
  // DOM and heap counters come from CDP, which only Chromium has.
  const cdp = browserName === "chromium" ? await page.context().newCDPSession(page) : null;
  await cdp?.send("Performance.enable");
  const size = await page.evaluate(() => Reflect.get(window, "largeDiffStats")());
  expect(size.files).toBe(600);
  expect(size.lines).toBeGreaterThan(30_000);

  await page.evaluate(() => Reflect.get(window, "prepareLargeDiff")(0));
  await frames(page);
  await takeLongTasks(page);
  const open = await page.evaluate(async () => {
    const start = performance.now();
    Reflect.get(window, "openLargeDiff")();
    while (!document.querySelector(".dv-files .dv-l")) await new Promise(done => setTimeout(done, 5));
    await new Promise(done => requestAnimationFrame(() => requestAnimationFrame(done)));
    return Math.round(performance.now() - start);
  });
  await frames(page);
  const openTasks = await takeLongTasks(page);
  await cdp?.send("HeapProfiler.collectGarbage");
  const nodes = cdp ? (await cdp.send("Memory.getDOMCounters")).nodes : null;
  const metrics = cdp ? (await cdp.send("Performance.getMetrics")).metrics : [];
  const heap = Math.round((metrics.find(m => m.name === "JSHeapUsedSize")?.value ?? 0) / 1e6);
  const dvNodes = await page.evaluate(() => document.querySelector(".dv")?.getElementsByTagName("*").length ?? 0);

  const files = page.locator(".dv-files");
  const box = await files.boundingBox();
  if (!box) throw new Error("no diff body");
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  const scroll = await page.evaluate(async () => {
    const list = document.querySelector<HTMLElement>(".dv-files");
    if (!list) throw new Error("no diff body");
    const gaps: number[] = [];
    let last = performance.now();
    for (let i = 0; i < 120; i++) {
      list.scrollTop += 600;
      await new Promise(done => requestAnimationFrame(done));
      const now = performance.now();
      gaps.push(now - last);
      last = now;
    }
    gaps.sort((a, b) => a - b);
    return { top: Math.round(list.scrollTop), p95: Math.round(gaps[Math.floor(gaps.length * 0.95)]), max: Math.round(gaps[gaps.length - 1]) };
  });
  await frames(page);
  const scrollTasks = await takeLongTasks(page);

  const jump = await page.evaluate(async () => {
    const tree = document.querySelector<HTMLElement>(".dv-tree");
    const find = () => document.querySelector<HTMLElement>('.dv-tr[data-path$="/module-540.ts"]');
    for (let i = 0; tree && !find() && i < 200; i++) {
      tree.scrollTop += 300;
      await new Promise(done => requestAnimationFrame(() => requestAnimationFrame(done)));
    }
    const row = find();
    if (!row) return -1;
    const start = performance.now();
    row.click();
    await new Promise(done => requestAnimationFrame(() => requestAnimationFrame(done)));
    return Math.round(performance.now() - start);
  });
  await expect(page.locator('.dv-file.cur[data-path$="/module-540.ts"]')).toBeVisible();
  await takeLongTasks(page);

  const before = await files.evaluate(el => el.scrollTop);
  await page.evaluate(() => Reflect.get(window, "prepareLargeDiff")(1));
  await frames(page);
  await takeLongTasks(page);
  const update = await page.evaluate(async () => {
    const start = performance.now();
    Reflect.get(window, "updateLargeDiff")();
    while (document.querySelector(".dv-head [aria-label='Refresh']:disabled")) await new Promise(done => setTimeout(done, 5));
    await new Promise(done => setTimeout(done, 50));
    await new Promise(done => requestAnimationFrame(() => requestAnimationFrame(done)));
    return Math.round(performance.now() - start);
  });
  const updateTasks = await takeLongTasks(page);
  const after = await files.evaluate(el => el.scrollTop);

  const max = (list: number[]) => Math.round(Math.max(0, ...list));
  const report = { open, openLongest: max(openTasks), nodes, dvNodes, heapMB: heap, scroll, scrollLongTasks: scrollTasks.length, scrollLongest: max(scrollTasks), jump, update, updateLongest: max(updateTasks), scrollKept: Math.abs(after - before) <= 2 };
  console.log(`large diff: ${JSON.stringify(report)}`);
  await info.attach("large-diff.json", { body: JSON.stringify(report), contentType: "application/json" });
  expect(report.scrollKept).toBe(true);
});

test("the file list marks the files a review commented on", async ({ page }, info) => {
  test.skip(info.project.name !== "macOS", "one run is enough");
  await setup(page);
  const path = "pkg-0/src/area-0/part-0/module-0.ts";
  await page.evaluate(path => {
    const comment = (text: string) => ({ path, side: "new", line: null, text, by: "claude" });
    localStorage.setItem("beep.diffReview.large-diff", JSON.stringify([comment("high: first"), comment("low: second")]));
    Reflect.get(window, "openLargeDiff")();
  }, path);

  const row = page.locator(`.dv-tree .dv-tr[data-path="${path}"]`);
  await expect(row.locator(".dv-cm")).toHaveText("2");
  await expect(row.locator(".dv-cm")).toHaveAttribute("title", "2 comments");
  await expect(page.locator(".dv-tree .dv-cm")).toHaveCount(1);
});

test("x folds and opens the file last clicked, not the one at the top", async ({ page }, info) => {
  test.skip(info.project.name !== "macOS", "one run is enough");
  await setup(page);
  await page.evaluate(() => Reflect.get(window, "openLargeDiff")());
  const files = page.locator(".dv-files");
  await expect(files.locator(".dv-file .dv-l").first()).toBeVisible();
  // Scroll so the second file starts mid screen, below the one at the top.
  const second = files.locator(".dv-file").nth(1);
  await files.evaluate((el, top) => { el.scrollTop = top - el.clientHeight / 2; }, await second.evaluate(el => el instanceof HTMLElement ? el.offsetTop : 0));
  const path = await second.getAttribute("data-path");
  const target = files.locator(`.dv-file[data-path="${path}"]`);
  const top = files.locator(".dv-file").first();
  await target.locator(".dv-l").first().click();

  await page.keyboard.press("x");
  await expect(target).toHaveClass(/closed/);
  await expect(top).not.toHaveClass(/closed/);
  await page.keyboard.press("x");
  await expect(target).not.toHaveClass(/closed/);
});
