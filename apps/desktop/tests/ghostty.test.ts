import { test } from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import zlib from "node:zlib";
import { Attr, Ghostty, loadGhostty, RenderState, RowCells, Terminal, type TerminalEffects } from "../src/stores/terminals/ghostty.ts";

const WASM = new URL("../ghostty/ghostty-vt.wasm", import.meta.url);
const SUM = new URL("../ghostty/ghostty-vt.wasm.sha256", import.meta.url);
const CELL = { width: 8, height: 16 };

const engine: Promise<Ghostty> = WebAssembly.compile(readFileSync(WASM)).then(loadGhostty);

interface Heard {
  replies: string[];
  bells: number;
  titles: number;
}

async function open(cols = 20, rows = 5): Promise<{ term: Terminal; state: RenderState; heard: Heard }> {
  const g = await engine;
  const heard: Heard = { replies: [], bells: 0, titles: 0 };
  const effects: TerminalEffects = {
    reply: bytes => heard.replies.push(new TextDecoder().decode(bytes)),
    bell: () => heard.bells++,
    titleChanged: () => heard.titles++,
  };
  const term = new Terminal(g, cols, rows, effects);
  term.resize(cols, rows, CELL);
  return { term, state: new RenderState(g), heard };
}

function write(term: Terminal, text: string) {
  term.write(new TextEncoder().encode(text));
}

/// Every row's text and a copy of its cells, after an update.
function rows(state: RenderState, term: Terminal): { text: string[]; cells: RowCells[] } {
  state.update(term);
  const text: string[] = [];
  const cells: RowCells[] = [];
  state.forEachRow(true, new RowCells(0), (y, row) => {
    let line = "";
    for (let x = 0; x < row.cols; x++) {
      if (row.attrs[x] & Attr.WIDE_TAIL) continue;
      line += row.graphemes.get(x) ?? (row.codepoint[x] === 0 ? " " : String.fromCodePoint(row.codepoint[x]));
    }
    text[y] = line.trimEnd();

    const copy = new RowCells(row.cols);
    copy.fit(row.cols);
    copy.codepoint.set(row.codepoint.subarray(0, row.cols));
    copy.fg.set(row.fg.subarray(0, row.cols));
    copy.bg.set(row.bg.subarray(0, row.cols));
    copy.attrs.set(row.attrs.subarray(0, row.cols));
    for (const [x, g] of row.graphemes) copy.graphemes.set(x, g);
    cells[y] = copy;
  });
  state.clean();
  return { text, cells };
}

test("the committed module matches its checksum", () => {
  const [sum] = readFileSync(SUM, "utf8").split(/\s+/);
  assert.equal(createHash("sha256").update(readFileSync(WASM)).digest("hex"), sum);
});

test("text and colours reach the rows", async () => {
  const { term, state } = await open();
  write(term, "Hello \x1b[1;31mred\x1b[0m\r\nnext");

  const { text, cells } = rows(state, term);
  assert.deepEqual(text.slice(0, 2), ["Hello red", "next"]);

  const colors = state.currentColors();
  const red = cells[0];
  assert.ok(red.attrs[6] & Attr.BOLD);
  assert.equal(red.fg[6], colors.palette[1]);
  assert.equal(red.fg[0], colors.foreground);
  assert.equal(red.attrs[0] & Attr.BOLD, 0);
  term.free();
  state.free();
});

test("truecolour, inverse video and underline styles decode", async () => {
  const { term, state } = await open();
  write(term, "\x1b[38;2;255;128;0mo\x1b[0m\x1b[7mi\x1b[0m\x1b[4:3mc\x1b[0m\x1b[48;5;4m \x1b[0m");

  const { cells } = rows(state, term);
  const row = cells[0];
  const colors = state.currentColors();
  assert.equal(row.fg[0], 0xff8000);
  assert.equal(row.fg[1], colors.background);
  assert.equal(row.bg[1], colors.foreground);
  assert.equal(row.underline(2), "curly");
  assert.equal(row.bg[3], colors.palette[4]);
  term.free();
  state.free();
});

test("wide characters and graphemes keep their cells", async () => {
  const { term, state } = await open();
  write(term, "漢a👍🏽b");

  const { text, cells } = rows(state, term);
  assert.equal(text[0], "漢a👍🏽b");
  assert.ok(cells[0].attrs[0] & Attr.WIDE);
  assert.ok(cells[0].attrs[1] & Attr.WIDE_TAIL);
  assert.equal(cells[0].graphemes.get(3), "👍🏽");
  term.free();
  state.free();
});

test("queries are answered through reply, apart from input", async () => {
  const { term, heard } = await open();
  write(term, "\x1b[6n\x1b[c");

  assert.equal(heard.replies.length, 2);
  assert.equal(heard.replies[0], "\x1b[1;1R");
  assert.match(heard.replies[1], /^\x1b\[\?[\d;]+c$/);
  term.free();
});

test("size queries are answered in cells and pixels, once the cell size is known", async () => {
  const g = await engine;
  const replies: string[] = [];
  const term = new Terminal(g, 20, 5, { reply: bytes => replies.push(new TextDecoder().decode(bytes)), bell: () => {}, titleChanged: () => {} });
  write(term, "\x1b[16t");
  assert.deepEqual(replies, []);

  term.resize(30, 10, CELL);
  write(term, "\x1b[16t\x1b[14t\x1b[18t");
  assert.deepEqual(replies, [
    `\x1b[6;${CELL.height};${CELL.width}t`,
    `\x1b[4;${10 * CELL.height};${30 * CELL.width}t`,
    "\x1b[8;10;30t",
  ]);
  term.free();
});

test("the bell and title reach the effects", async () => {
  const { term, heard } = await open();
  write(term, "\x07\x1b]2;build\x07");

  assert.equal(heard.bells, 1);
  assert.equal(heard.titles, 1);
  assert.equal(term.title(), "build");
  term.free();
});

test("only changed rows are dirty after a clean frame", async () => {
  const { term, state } = await open();
  write(term, "one\r\ntwo\r\nthree");
  rows(state, term);

  write(term, "\x1b[2;1Hxx");
  assert.equal(state.update(term), "partial");
  const dirty: number[] = [];
  state.forEachRow(false, new RowCells(0), y => dirty.push(y));
  state.clean();
  // Row 1 changed, and row 2 lost the cursor.
  assert.deepEqual(dirty, [1, 2]);
  assert.equal(state.update(term), "clean");
  term.free();
  state.free();
});

test("the cursor follows output", async () => {
  const { term, state } = await open();
  write(term, "ab\r\ncd\x1b[5 q");

  state.update(term);
  assert.deepEqual(state.cursor(), { x: 2, y: 1, shape: "bar", blinking: true, wideTail: false });
  write(term, "\x1b[?25l");
  state.update(term);
  assert.equal(state.cursor(), null);
  term.free();
  state.free();
});

test("resizing and scrolling move what is shown", async () => {
  const { term, state } = await open(10, 3);
  for (let i = 0; i < 10; i++) write(term, `line ${i}\r\n`);
  assert.equal(rows(state, term).text[0], "line 8");

  term.scrollBy(-2);
  assert.equal(rows(state, term).text[0], "line 6");
  term.scrollToBottom();
  assert.equal(rows(state, term).text[0], "line 8");

  term.resize(30, 6, CELL);
  assert.deepEqual(term.size(), { cols: 30, rows: 6 });
  term.free();
  state.free();
});

/// A width by height PNG of one RGBA colour, built with Node's zlib.
function png(width: number, height: number, rgba: [number, number, number, number]): Buffer {
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
  header.set([8, 6, 0, 0, 0], 8);
  const row = Buffer.from([0, ...Array.from({ length: width }, () => rgba).flat()]);
  const raw = Buffer.concat(Array.from({ length: height }, () => row));
  return Buffer.concat([
    Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]),
    chunk("IHDR", header),
    chunk("IDAT", zlib.deflateSync(raw)),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

test("kitty images, PNG included, are placed and read back", async () => {
  const { term, state, heard } = await open();
  const data = png(4, 2, [255, 0, 0, 255]).toString("base64");
  // As yazi sends one: PNG, placed over 2 by 1 cells at the cursor.
  write(term, `\x1b[2;3H\x1b_Ga=T,f=100,i=31,c=2,r=1,q=2;${data}\x1b\\`);
  assert.deepEqual(heard.replies, []);

  state.update(term);
  const placements = state.placements();
  assert.equal(placements.length, 1);
  const [p] = placements;
  assert.equal(p.id, 31);
  assert.deepEqual([p.col, p.row, p.width, p.height], [2, 1, 2 * CELL.width, CELL.height]);
  assert.deepEqual(p.source, { x: 0, y: 0, width: 4, height: 2 });
  assert.equal(p.layer, "above-text");

  const pixels = term.imagePixels(31);
  assert.ok(pixels);
  assert.deepEqual([pixels.width, pixels.height], [4, 2]);
  assert.deepEqual([...pixels.rgba.subarray(0, 4)], [255, 0, 0, 255]);
  assert.equal(term.imagePixels(99), null);

  // Programs ask whether images work before sending them.
  write(term, "\x1b_Gi=1,a=q,s=1,v=1,f=24;AAAA\x1b\\");
  assert.match(heard.replies[0] ?? "", /^\x1b_Gi=1;OK\x1b\\$/);
  term.free();
  state.free();
});

test("a terminal keeps images only up to its limit", async () => {
  const g = await engine;
  const term = new Terminal(g, 20, 5, { reply() {}, bell() {}, titleChanged() {} }, { images: 40, scrollback: 100_000 });
  term.resize(20, 5, CELL);
  // Each is 4 by 2 pixels, 32 bytes: the second pushes the first out.
  for (const id of [1, 2]) write(term, `\x1b_Ga=T,f=100,i=${id},c=2,r=1,q=2;${png(4, 2, [255, 0, 0, 255]).toString("base64")}\x1b\\`);
  assert.equal(term.imagePixels(1), null);
  assert.ok(term.imagePixels(2));
  term.free();
});

test("theme colours become the terminal's defaults", async () => {
  const { term, state } = await open();
  term.setColors({ foreground: 0xcfe3d8, background: 0x0b0f0d, cursor: 0x6fcf97, palette: new Map([[2, 0x6fcf97]]) });
  write(term, "a\x1b[32mb");

  const { cells } = rows(state, term);
  const colors = state.currentColors();
  assert.equal(colors.background, 0x0b0f0d);
  assert.equal(colors.cursor, 0x6fcf97);
  assert.equal(cells[0].fg[0], 0xcfe3d8);
  assert.equal(cells[0].fg[1], 0x6fcf97);
  term.free();
  state.free();
});
