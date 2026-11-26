import { expect, test, type Page } from "@playwright/test";
import { build } from "esbuild";
import { readFile } from "node:fs/promises";
import { extname, isAbsolute, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import zlib from "node:zlib";

// A terminal screen (termview.ts) on its own page: Ghostty's engine from
// dist/, the GPU renderer, keys and images, as the app's windows run them.
const desktop = fileURLToPath(new URL("../../", import.meta.url));
const dist = resolve(desktop, "dist");

/// The app's native calls, which the screen makes only to log: kept for the test.
const NATIVE = `window.__TAURI__ = { core: { invoke: async (command, args) => { (window.logged ??= []).push(args); return null; }, Channel: class {} }, event: { listen: async () => () => {} } };`;

/// The screen bundled as a page script, with hooks for the test.
const ENTRY = `
import { makeScreen } from "./src/terminals/termview.ts";
const typed = [];
const replies = [];
const screen = makeScreen();
screen.el.style.height = "400px";
screen.el.style.width = "800px";
document.body.append(screen.el);
screen.onData(data => typed.push(data));
screen.onReply(data => replies.push(data));
screen.setInput(true);
screen.fit();
window.term = {
  screen,
  typed,
  replies,
  write: text => screen.write(new TextEncoder().encode(text)),
  replay: text => screen.replay(new TextEncoder().encode(text)),
  // Pixels of a PNG screenshot of the screen, counted by colour. A WebGL
  // canvas cannot be read back once shown, so the test takes the picture.
  async count(png, rgb) {
    const bitmap = await createImageBitmap(new Blob([Uint8Array.from(atob(png), c => c.charCodeAt(0))], { type: "image/png" }));
    const copy = document.createElement("canvas");
    copy.width = bitmap.width;
    copy.height = bitmap.height;
    const ctx = copy.getContext("2d");
    ctx.drawImage(bitmap, 0, 0);
    const data = ctx.getImageData(0, 0, copy.width, copy.height).data;
    let n = 0;
    for (let i = 0; i < data.length; i += 4) {
      if (Math.abs(data[i] - rgb[0]) < 8 && Math.abs(data[i + 1] - rgb[1]) < 8 && Math.abs(data[i + 2] - rgb[2]) < 8) n++;
    }
    return n;
  },
};
`;

declare global {
  interface Window {
    term: {
      screen: { el: HTMLElement; size(): { cols: number; rows: number }; focus(): void; fit(): void };
      typed: string[];
      replies: string[];
      write(text: string): void;
      replay(text: string): void;
      count(png: string, rgb: [number, number, number]): Promise<number>;
    };
  }
}

async function open(page: Page) {
  const bundle = await build({
    stdin: { contents: ENTRY, resolveDir: desktop, loader: "ts" },
    bundle: true, format: "esm", write: false, target: "safari16", banner: { js: NATIVE },
  });
  const script = bundle.outputFiles[0].text;
  const types = new Map([[".css", "text/css"], [".wasm", "application/wasm"], [".woff2", "font/woff2"]]);

  await page.route("https://app.test/**", async route => {
    const path = decodeURIComponent(new URL(route.request().url()).pathname).replace(/^\/+/, "");
    if (path === "term.html") {
      return route.fulfill({
        contentType: "text/html",
        body: `<!doctype html><link rel="stylesheet" href="fonts.css"><link rel="stylesheet" href="style.css"><link rel="stylesheet" href="skins.css"><body><script type="module" src="term.js"></script></body>`,
      });
    }
    if (path === "term.js") return route.fulfill({ contentType: "text/javascript", body: script });

    const file = resolve(dist, path);
    const within = relative(dist, file);
    if (within.startsWith("..") || isAbsolute(within)) return route.fulfill({ status: 403 });
    try { await route.fulfill({ body: await readFile(file), contentType: types.get(extname(file)) ?? "application/octet-stream" }); }
    catch { await route.fulfill({ status: 404 }); }
  });

  await page.goto("https://app.test/term.html");
  await page.waitForFunction(() => window.term !== undefined);
  // The engine loads, then the grid fits the element.
  await page.waitForFunction(() => window.term.screen.size().cols !== 80);
}

/// Pixels of `rgb` on the screen as it shows now.
async function pixels(page: Page, rgb: [number, number, number]): Promise<number> {
  const png = (await page.locator("canvas").screenshot()).toString("base64");
  return page.evaluate(({ png, rgb }) => window.term.count(png, rgb), { png, rgb });
}

/// A PNG of one colour, as yazi would send a picture.
function png(width: number, height: number, rgb: [number, number, number]): string {
  const chunk = (type: string, data: Buffer) => {
    const len = Buffer.alloc(4);
    len.writeUInt32BE(data.length);
    const body = Buffer.concat([Buffer.from(type, "ascii"), data]);
    const crc = Buffer.alloc(4);
    crc.writeUInt32BE(zlib.crc32(body));
    return Buffer.concat([len, body, crc]);
  };
  const header = Buffer.alloc(13);
  header.writeUInt32BE(width, 0);
  header.writeUInt32BE(height, 4);
  header.set([8, 2, 0, 0, 0], 8);
  const row = Buffer.from([0, ...Array.from({ length: width }, () => rgb).flat()]);
  const raw = Buffer.concat(Array.from({ length: height }, () => row));
  return Buffer.concat([
    Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk("IHDR", header), chunk("IDAT", zlib.deflateSync(raw)), chunk("IEND", Buffer.alloc(0)),
  ]).toString("base64");
}

test("the screen fits its element and draws text, colours and images, placeholders included", async ({ page }) => {
  await open(page);
  const size = await page.evaluate(() => window.term.screen.size());
  expect(size.cols).toBeGreaterThan(40);
  expect(size.rows).toBeGreaterThan(10);

  const blue: [number, number, number] = [20, 60, 230];
  const image = png(64, 64, blue);
  await page.evaluate(image => {
    window.term.write("\x1b[48;2;200;30;30m          \x1b[0m hello\r\n");
    window.term.write(`\x1b_Ga=T,f=100,i=7,c=4,r=2,q=2;${image}\x1b\\`);
  }, image);

  await expect.poll(() => pixels(page, [200, 30, 30])).toBeGreaterThan(500);
  await expect.poll(() => pixels(page, blue)).toBeGreaterThan(500);

  // Placed by Unicode placeholders, as yazi does: the image's cells show it,
  // and no glyph of the placeholder character.
  const green: [number, number, number] = [30, 200, 60];
  await page.evaluate(image => {
    const cell = (row: string, col: string) => `\u{10EEEE}${row}${col}`;
    const [r0, r1, c0, c1] = ["\u0305", "\u030D", "\u0305", "\u030D"];
    window.term.write(`\r\n\r\n\x1b_Ga=t,f=100,i=9,q=2;${image}\x1b\\\x1b_Ga=p,U=1,i=9,c=2,r=2,q=2\x1b\\`);
    window.term.write(`\x1b[38;5;9m${cell(r0, c0)}${cell(r0, c1)}\r\n${cell(r1, c0)}${cell(r1, c1)}\x1b[0m\r\n`);
  }, png(64, 64, green));
  await expect.poll(() => pixels(page, green)).toBeGreaterThan(180);
  // Nerd Font icons come from the bundled symbols font.
  const loaded = await page.evaluate(async () => (await document.fonts.load("12px 'Symbols Nerd Font Mono'", "\u{f07b}")).map(face => face.status));
  expect(loaded).toEqual(["loaded"]);

  // Several lines retain enough foreground pixels with either browser's font rasterizer.
  await page.evaluate(() => window.term.write("the quick brown fox jumps over the lazy dog\r\n".repeat(8)));
  await expect.poll(() => pixels(page, [0xe3, 0xe4, 0xe6])).toBeGreaterThan(20);
});

test("a screen hidden and shown again draws again", async ({ page }) => {
  await open(page);
  const red: [number, number, number] = [200, 30, 30];

  // As when another terminal is opened and this one is opened again.
  for (let i = 0; i < 3; i++) {
    await page.evaluate(() => { window.term.screen.el.style.display = "none"; window.term.screen.fit(); });
    await page.evaluate(() => { window.term.screen.el.style.display = ""; window.term.screen.fit(); });
  }
  await page.evaluate(() => window.term.write("\x1b[48;2;200;30;30m          \x1b[0m\r\n"));

  await expect.poll(() => pixels(page, red)).toBeGreaterThan(500);
});

test("a pane that narrows narrows the grid", async ({ page }) => {
  await open(page);
  // As in the app: the screen fills a flex item in a row of panes.
  await page.evaluate(() => {
    const row = document.createElement("div");
    row.style.cssText = "display: flex; width: 800px; height: 400px";
    const pane = document.createElement("div");
    pane.style.cssText = "flex: 1; display: flex; flex-direction: column";
    const el = window.term.screen.el;
    el.style.cssText = "flex: 1";
    pane.append(el);
    row.append(pane);
    document.body.append(row);
  });
  await expect.poll(() => page.evaluate(() => window.term.screen.size().cols)).toBeGreaterThan(80);
  const wide = await page.evaluate(() => window.term.screen.size().cols);

  await page.evaluate(() => { (window.term.screen.el.parentElement?.parentElement as HTMLElement).style.width = "400px"; });
  await expect.poll(() => page.evaluate(() => window.term.screen.size().cols)).toBeLessThan(wide / 2 + 2);
  const fits = await page.evaluate(() => window.term.screen.el.querySelector("canvas")!.getBoundingClientRect().right <= window.term.screen.el.getBoundingClientRect().right);
  expect(fits).toBe(true);
});

test("a steady cursor stays drawn", async ({ page }) => {
  await open(page);
  await page.evaluate(() => { window.term.screen.focus(); window.term.write("\x1b[2 q"); });
  const cursor: [number, number, number] = [0x5e, 0x6a, 0xd2];

  // Across several blink periods.
  for (let i = 0; i < 6; i++) {
    expect(await pixels(page, cursor)).toBeGreaterThan(40);
    await page.waitForTimeout(200);
  }
});

test("keys go out encoded, apart from the terminal's replies", async ({ page }) => {
  await open(page);
  await page.evaluate(() => window.term.screen.focus());
  await page.keyboard.type("ls");
  await page.keyboard.press("Enter");
  await page.keyboard.press("ArrowUp");
  await expect.poll(() => page.evaluate(() => window.term.typed.join(""))).toBe("ls\r\x1b[A");

  await page.evaluate(() => window.term.write("\x1b[?1h\x1b[6n"));
  await expect.poll(() => page.evaluate(() => window.term.replies.join(""))).toMatch(/^\x1b\[\d+;\d+R$/);
  await page.keyboard.press("ArrowUp");
  await expect.poll(() => page.evaluate(() => window.term.typed.at(-1))).toBe("\x1bOA");
});

test("quadrants and Powerline glyphs fill their cells as shapes", async ({ page }) => {
  await open(page);
  const red: [number, number, number] = [200, 30, 30];
  /// Red pixels with 20 of `glyph` on an otherwise clear screen.
  const area = async (glyph: string) => {
    await page.evaluate(g => window.term.write(`\x1b[2J\x1b[H\x1b[38;2;200;30;30m${g.repeat(20)}\x1b[0m`), glyph);
    let last = -1;
    // Settled: two screenshots in a row agree.
    await expect.poll(async () => { const now = await pixels(page, red); const same = now === last; last = now; return same; }).toBe(true);
    return last;
  };

  const full = await area("█");
  expect(full).toBeGreaterThan(500);
  // A quarter, and three quarters that meet the next cell's.
  expect(Math.abs(await area("▘") / full - 0.25)).toBeLessThan(0.05);
  expect(Math.abs(await area("▛") / full - 0.75)).toBeLessThan(0.05);
  // A triangle is half its cell, and stays in it though the next is blank.
  expect(Math.abs(await area(" ") / full - 0.5)).toBeLessThan(0.08);
});

test("rows drawn before the glyph atlas fills keep their glyphs", async ({ page }) => {
  await open(page);
  const red: [number, number, number] = [200, 30, 30];
  // A red bar on the top row, drawn once and then left alone.
  await page.evaluate(() => window.term.write("\x1b[2J\x1b[H\x1b[38;2;200;30;30m" + "█".repeat(40) + "\x1b[0m"));
  await expect.poll(() => pixels(page, red)).toBeGreaterThan(500);
  const before = await pixels(page, red);

  // Rows below it rewritten with new characters, a screen at a time, until
  // the atlas has had to clear: more distinct glyphs than it holds. The bar
  // looks the same after every screen, the one that cleared the atlas too.
  let cp = 0x4e00;
  for (let screen = 0; screen < 24; screen++) {
    cp = await page.evaluate(async start => {
      const { cols, rows } = window.term.screen.size();
      let cp = start;
      let text = "";
      for (let y = 2; y <= rows; y++) {
        text += `\x1b[${y};1H`;
        // Wide characters: half a row of them, so nothing wraps or scrolls.
        for (let x = 0; x < cols / 2 - 1; x++) text += String.fromCodePoint(cp++);
      }
      window.term.write(text);
      await new Promise(done => requestAnimationFrame(() => requestAnimationFrame(done)));
      return cp;
    }, cp);
    expect(await pixels(page, red), `after screen ${screen}`).toBe(before);
  }
});

test("queries in replayed output go unanswered, live ones are answered", async ({ page }) => {
  await open(page);
  // Primary device attributes, as a shell asks at start.
  await page.evaluate(() => window.term.replay("old prompt\x1b[c"));
  await page.waitForTimeout(200);
  expect(await page.evaluate(() => window.term.replies)).toEqual([]);

  await page.evaluate(() => window.term.write("\x1b[c"));
  await expect.poll(() => page.evaluate(() => window.term.replies.length)).toBe(1);
  expect(await page.evaluate(() => window.term.replies[0])).toMatch(/^\x1b\[\?/);
});
