// PLAYWRIGHT_DIR=/path/with/node_modules/ node tests/perf/diff-scroll.mjs
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { buildSync } from "esbuild";
import { playwright, stubInit } from "./stub.mjs";

const code = buildSync({
  entryPoints: [new URL("../../src/review/diff.ts", import.meta.url).pathname],
  bundle: true, write: false, format: "iife", globalName: "diffViewer",
}).outputFiles[0].text;
const css = readFileSync(new URL("../../dist/diff.css", import.meta.url), "utf8");
const arg = process.argv.indexOf("--browser");
const kinds = arg < 0 ? ["webkit", "chromium"] : [process.argv[arg + 1]];
for (const kind of kinds) {
  const executablePath = (kind === "webkit" ? process.env.WEBKIT_BIN : process.env.CHROMIUM_BIN) || undefined;
  const browser = await playwright()[kind].launch({ executablePath });
  try {
    const page = await browser.newPage({ viewport: { width: 1280, height: 800 }, reducedMotion: "reduce" });
    const errors = [];
    page.on("pageerror", e => errors.push(e.message));
    await page.setContent('<style>body { margin: 0; --mono: monospace } #host { width: 1200px; height: 700px } [hidden] { display: none !important }</style><div id="bar" data-mode="window" hidden></div><div id="host"></div>');
    await page.addStyleTag({ content: css });
    await page.evaluate(stubInit, JSON.stringify({ agent_hosts: [] }));
    await page.addScriptTag({ content: code });
    await page.evaluate(() => {
      window.openDiff = (count, lines, readOnly = true) => {
        const files = Array.from({ length: count }, (_, i) => ({
          path: `src/file-${i}.ts`, old_path: null, status: "modified", additions: lines, deletions: 0, left_out: null,
          hunks: [{ old_start: 1, old_lines: 0, new_start: 1, new_lines: lines, section: "", lines: Array.from({ length: lines }, (_, n) => ({ k: "+", t: `const value${n} = "${"example ".repeat(30)}";` })) }],
        }));
        const v = diffViewer.diffView({ readOnly,
          load: async () => ({ root: "/project", base: "main", merge_base: "", working_tree: false, files, additions: count * lines, deletions: 0, more: 0 }),
          review: { key: "scroll-test", heading: () => "Review", send: async () => "" },
        });
        document.querySelector("#host").replaceChildren(v.el);
      };
      window.openDiff(3, 40);
    });
    await page.waitForSelector(".dv-chunk .dv-l");
    await page.evaluate(() => new Promise(done => requestAnimationFrame(() => requestAnimationFrame(done))));
    // Wheel over code inside the file's horizontal scroller and fold.
    await page.mouse.move(700, 250);
    const before = await page.locator(".dv-files").evaluate(e => e.scrollTop);
    await page.mouse.wheel(0, 240);
    await page.waitForTimeout(250);
    const down = await page.locator(".dv-files").evaluate(e => e.scrollTop);
    console.log(`${kind}: wheel down ${down - before}px`);
    assert.ok(down - before >= 200, `${kind}: code must pass the vertical wheel movement to the diff`);
    await page.mouse.wheel(0, -180);
    await page.waitForTimeout(250);
    const up = await page.locator(".dv-files").evaluate(e => e.scrollTop);
    console.log(`${kind}: wheel up ${down - up}px`);
    assert.ok(down - up >= 150, `${kind}: code must pass upward wheel movement to the diff`);
    await page.mouse.wheel(200, 0);
    await page.waitForFunction(() => document.querySelector(".dv-scroll").scrollLeft >= 150);

    await page.evaluate(() => window.openDiff(150, 300));
    await page.waitForSelector(".dv-chunk .dv-l");
    const stats = await page.evaluate(async () => {
      const b = document.querySelector(".dv-files");
      const scroll = Object.getOwnPropertyDescriptor(Element.prototype, "scrollTop");
      const rect = Element.prototype.getBoundingClientRect;
      const rects = Element.prototype.getClientRects;
      let reads = 0, writes = 0, ms = 0;
      Element.prototype.getBoundingClientRect = function () { reads++; return rect.call(this); };
      Element.prototype.getClientRects = function () { reads++; return rects.call(this); };
      Object.defineProperty(b, "scrollTop", { configurable: true,
        get() { return scroll.get.call(this); },
        set(value) { writes++; scroll.set.call(this, value); },
      });
      for (const top of [600, 1200, 1800, 2400, 2000, 1200, 600]) {
        scroll.set.call(b, top);
        const start = performance.now();
        b.dispatchEvent(new Event("scroll"));
        await new Promise(done => requestAnimationFrame(done));
        ms += performance.now() - start;
        if (b.scrollTop !== top) throw new Error(`Scroll moved from ${top} to ${b.scrollTop}`);
      }
      Element.prototype.getBoundingClientRect = rect;
      Element.prototype.getClientRects = rects;
      return { reads, writes, ms: Math.round(ms), rows: b.querySelectorAll(".dv-l").length };
    });
    console.log(`${kind}: ${JSON.stringify(stats)}`);
    assert.equal(stats.writes, 0, `${kind}: unchanged chunk heights must not interrupt wheel glide`);
    assert.ok(stats.reads < 2000, `${kind}: scrolling must not measure every offscreen chunk`);
    assert.ok(stats.rows < 250, `${kind}: only nearby rows should be mounted`);

    const filled = () => page.waitForFunction(() => {
      const b = document.querySelector(".dv-files"), view = b.getBoundingClientRect();
      const chunks = [...b.querySelectorAll(".dv-file:not(.closed):not([hidden]) .dv-chunk")].filter(c => {
        const r = c.getBoundingClientRect();
        return r.bottom > view.top && r.top < view.bottom;
      });
      return chunks.length && chunks.every(c => c.childElementCount);
    });
    await page.locator('.dv-tr[data-path="src/file-70.ts"]').evaluate(e => e.click());
    await filled();
    assert.equal(await page.locator(".dv-file.cur").getAttribute("data-path"), "src/file-70.ts");
    await page.locator('.dv-file.cur .dv-fh').click();
    await page.waitForSelector('.dv-file.cur.closed');
    await page.locator('.dv-file.cur .dv-fh').click();
    await filled();
    await page.locator('.dv-find').fill("file-90.ts");
    await filled();
    assert.equal(await page.locator(".dv-file:not([hidden])").count(), 1);
    await page.locator('.dv-splitbtn').click();
    await filled();
    assert.ok(await page.locator('.dv-file:not([hidden]) .dv-pair').count() > 0);
    await page.locator('#host').evaluate(e => e.style.width = "600px");
    await page.waitForFunction(() => !document.querySelector('.dv-file:not([hidden]) .dv-lines').classList.contains("split"));
    await filled();
    console.log(`${kind}: file jumps, folding, filtering and split resizing render the visible chunks`);

    await page.locator('#host').evaluate(e => e.style.width = "1200px");
    await page.evaluate(() => window.openDiff(3, 300, false));
    await filled();
    await page.locator('.dv-file').first().locator('.dv-l.add .dv-g').first().click();
    await page.locator('.dv-cmt.open textarea').fill("Keep this draft while scrolling.");
    await page.locator('.dv-files').evaluate(e => e.scrollTop = 12000);
    await filled();
    assert.equal(await page.locator('.dv-cmt.open textarea').inputValue(), "Keep this draft while scrolling.");
    await page.locator('.dv-files').evaluate(e => e.scrollTop = 0);
    await filled();
    await page.locator('.dv-cmt.open button').filter({ hasText: "Cancel" }).click();
    const selection = await page.locator('.dv-file').first().locator('.dv-l.add .dv-c').first().evaluate(e => {
      const range = document.createRange();
      range.selectNodeContents(e);
      document.getSelection().removeAllRanges();
      document.getSelection().addRange(range);
      return document.getSelection().toString();
    });
    await page.locator('.dv-files').evaluate(e => e.scrollTop = 12000);
    await filled();
    assert.equal(await page.evaluate(() => document.getSelection().toString()), selection);
    console.log(`${kind}: offscreen comments and selected text survive chunk eviction`);
    assert.deepEqual(errors, []);
  } finally { await browser.close(); }
}
