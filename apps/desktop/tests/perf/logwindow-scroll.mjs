// Check scroll writes in a real layout, including a reply taller than the view.
// PLAYWRIGHT_DIR=/path/with/node_modules/ node tests/perf/logwindow-scroll.mjs
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { buildSync } from "esbuild";
import { playwright } from "./stub.mjs";

const code = buildSync({
  entryPoints: [new URL("../../src/parts/session/logwindow.tsx", import.meta.url).pathname],
  bundle: true, write: false, format: "iife", globalName: "logWindow",
}).outputFiles[0].text;

const browserArg = process.argv.indexOf("--browser");
const kinds = browserArg < 0 ? ["webkit", "chromium"] : [process.argv[browserArg + 1]];

for (const kind of kinds) {
  const executablePath = (kind === "webkit" ? process.env.WEBKIT_BIN : process.env.CHROMIUM_BIN) || undefined;
  const browser = await playwright()[kind].launch({ executablePath });

  try {
    const page = await browser.newPage();
    await page.setContent(`<style>
      .dbody { height: 500px; overflow: auto; overflow-anchor: none }
      .dcoderlog, .agturn { display: flex; flex-direction: column; gap: 12px }
      [hidden] { display: none }
      .agspace { flex: none }
    </style><div class="dbody"><div class="dcoderlog"></div></div>`);
    await page.addScriptTag({ content: code });
    await page.evaluate(() => {
      const b = document.querySelector(".dbody"), log = b.firstElementChild;
      b.stuck = false;
      logWindow.setUnits(log, [{ key: "long-reply", rows: () => ['<div style="height:6000px">Long reply</div>'] }], []);
      logWindow.fillLog(b, false);
    });
    // Let the initial ResizeObserver measurement settle before recording writes.
    await page.evaluate(() => new Promise(done => requestAnimationFrame(() => requestAnimationFrame(done))));
    const writes = await page.evaluate(() => {
      const b = document.querySelector(".dbody");
      const scroll = Object.getOwnPropertyDescriptor(Element.prototype, "scrollTop");
      let writes = 0;
      Object.defineProperty(b, "scrollTop", {
        configurable: true,
        get() { return scroll.get.call(this); },
        set(value) { writes++; scroll.set.call(this, value); },
      });

      // The browser has already applied a wheel movement when fillLog runs.
      for (const top of [1500, 1400, 1300, 1400, 1500]) {
        scroll.set.call(b, top);
        logWindow.fillLog(b, false);
        if (b.scrollTop !== top) throw new Error(`Scroll moved from ${top} to ${b.scrollTop}`);
      }

      return writes;
    });
    assert.equal(writes, 0, `${kind}: unchanged turns must not interrupt native scrolling with scrollTop writes`);
    console.log(`${kind}: scrolling within a long reply makes no scroll writes`);

    await page.evaluate(() => {
      const b = document.querySelector(".dbody");
      b.addEventListener("scroll", () => logWindow.fillLog(b, false));
    });
    await page.mouse.move(200, 200);
    await page.mouse.wheel(0, 300);
    await page.waitForFunction(() => document.querySelector(".dbody").scrollTop > 1500);
    const down = await page.locator(".dbody").evaluate(b => b.scrollTop);
    await page.mouse.wheel(0, -300);
    await page.waitForFunction(down => document.querySelector(".dbody").scrollTop < down, down);
    console.log(`${kind}: wheel scrolling moves both down and up`);

    const growthWrites = await page.evaluate(async () => {
      const b = document.querySelector(".dbody");
      const scroll = Object.getOwnPropertyDescriptor(Element.prototype, "scrollTop");
      let writes = 0;
      Object.defineProperty(b, "scrollTop", {
        configurable: true,
        get() { return scroll.get.call(this); },
        set(value) { writes++; scroll.set.call(this, value); },
      });
      b.querySelector(".agturn > div").style.height = "6200px";
      await new Promise(done => requestAnimationFrame(() => requestAnimationFrame(done)));
      return writes;
    });
    assert.equal(growthWrites, 0, `${kind}: growth below the reader must not write the scroll position`);
    console.log(`${kind}: reply growth below the reader leaves native scrolling alone`);

    await page.evaluate(() => {
      const b = document.querySelector(".dbody"), log = b.firstElementChild;
      const scroll = Object.getOwnPropertyDescriptor(Element.prototype, "scrollTop");
      scroll.set.call(b, 0);
      logWindow.setUnits(log, Array.from({ length: 40 }, (_, i) => ({
        key: `turn-${i}`, rows: () => [`<div style="height:400px">Turn ${i}</div>`],
      })), []);
      logWindow.fillLog(b, false);

      for (const top of [600, 1200, 1800, 2400, 2000, 1200, 600]) {
        scroll.set.call(b, top);
        logWindow.fillLog(b, false);
        if (b.scrollTop !== top) throw new Error(`Crossing turns moved the scroll from ${top} to ${b.scrollTop}`);
      }
    });
    console.log(`${kind}: crossing virtualized turns preserves the scroll position`);

    await page.evaluate(() => {
      const b = document.querySelector(".dbody");
      b.stuck = true;
      logWindow.fillLog(b, true);
      b.style.height = "400px";
    });
    await page.waitForFunction(() => {
      const b = document.querySelector(".dbody");
      return b.scrollHeight - b.scrollTop - b.clientHeight < 1;
    });
    console.log(`${kind}: shrinking the viewport keeps the log at the bottom`);

    const heldTop = await page.locator(".dbody").evaluate(b => {
      b.stuck = false;
      b.scrollTop -= 200;
      b.style.height = "350px";
      return b.scrollTop;
    });
    await page.evaluate(() => new Promise(done => requestAnimationFrame(() => requestAnimationFrame(() => requestAnimationFrame(done)))));
    assert.equal(await page.locator(".dbody").evaluate(b => b.scrollTop), heldTop,
      `${kind}: resizing must preserve a reader's position above the bottom`);

    // Use the app's Markdown styles, including the nested overflow on code and tables.
    await page.setContent('<div class="dbody" id="agbody"><div class="dagmain"><div class="dcoderlog"></div></div></div>');
    const css = readFileSync(new URL("../../dist/style.css", import.meta.url), "utf8");
    await page.addStyleTag({ content: css.replace('@import url("skins.css");', "") });
    await page.addStyleTag({ content: ".dbody { position: fixed; top: 8px; left: 8px; width: 700px; height: 500px }" });
    await page.evaluate(() => {
      const b = document.querySelector(".dbody"), log = b.querySelector(".dcoderlog");
      const code = Array.from({ length: 30 }, (_, i) => `  \"field_${i}\": \"${"example ".repeat(20)}\"`).join("\n");
      const rows = Array.from({ length: 20 }, (_, i) => `<tr><td>Topic ${i}</td><td>${"Example cell ".repeat(10)}</td></tr>`).join("");
      const html = `<div class="dcoderen agent md"><p>Example reply</p><pre class="dcode"><button class="dcopy">Copy</button><code>{\n${code}\n}</code></pre><p>Example topics</p><table><tbody>${rows}</tbody></table><p>${"Trailing explanation. ".repeat(200)}</p></div>`;
      b.stuck = false;
      logWindow.setUnits(log, [{ key: "markdown", rows: () => [html] }], []);
      logWindow.fillLog(b, false);
      b.addEventListener("scroll", () => logWindow.fillLog(b, false));
    });

    for (const selector of ["pre.dcode", "table"]) {
      await page.locator(selector).evaluate(el => el.scrollIntoView({ block: "start" }));
      await page.evaluate(() => new Promise(done => requestAnimationFrame(() => requestAnimationFrame(done))));
      const before = await page.locator(".dbody").evaluate(b => b.scrollTop);
      await page.mouse.move(200, 100);
      await page.mouse.wheel(0, 160);
      await page.waitForFunction(before => document.querySelector(".dbody").scrollTop > before, before);
      const down = await page.locator(".dbody").evaluate(b => b.scrollTop);
      await page.mouse.wheel(0, -160);
      await page.waitForFunction(down => document.querySelector(".dbody").scrollTop < down, down);
    }

    console.log(`${kind}: wheel scrolling over Markdown code blocks and tables moves the conversation both ways`);
  } finally {
    await browser.close();
  }
}
