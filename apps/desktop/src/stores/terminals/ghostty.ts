// Ghostty's terminal engine (libghostty-vt), built to WebAssembly by
// scripts/build-ghostty-wasm.sh, behind typed classes: a Terminal that takes a
// program's output, and a RenderState that reads what to draw from it. Struct
// offsets, enum values and bit fields come from ghostty-abi.ts, generated from
// Ghostty's own description of its ABI.
//
// What the terminal answers a program with (query replies) arrives through
// TerminalEffects.reply, apart from anything typed: a node that answers
// queries itself can then drop the replies without touching input.
import {
  GhosttyCell,
  GhosttyCellContentTag,
  GhosttyCellWide,
  GhosttyCodepointBuffer,
  GhosttyCursorStyle,
  GhosttyImageLayer,
  GhosttyKittyGraphicsImageData,
  GhosttyKittyImageFormat,
  GhosttyRenderStateColors,
  GhosttyRenderStateCursor,
  GhosttyRenderStateData,
  GhosttyRenderStateDirty,
  GhosttyRenderStateImageData,
  GhosttyRenderStateOption,
  GhosttyRenderStateRowCellsData,
  GhosttyRenderStateRowData,
  GhosttyRenderStateRowOption,
  GhosttyRenderStateRowSelection,
  GhosttyResult,
  GhosttySgrUnderline,
  GhosttySizeReportSize,
  GhosttyString,
  GhosttyStyle,
  GhosttyStyleColor,
  GhosttyStyleColorTag,
  GhosttyStyleFlags,
  GhosttyTerminalData,
  GhosttyTerminalModeConfig,
  GhosttyTerminalOption,
  GhosttyTerminalScrollViewport,
  GhosttyTerminalScrollViewportTag,
} from "./ghostty-abi.ts";

/// What one terminal may keep, in bytes. Every terminal of a window shares
/// one wasm memory, which never shrinks, so these stay well under what a
/// webview allows even with several terminals at their limits.
export interface TerminalLimits {
  /// Kitty image data; Ghostty's own default is 320 MB.
  images: number;
  scrollback: number;
}

export const DESKTOP_LIMITS: TerminalLimits = { images: 64_000_000, scrollback: 10_000_000 };
/// A phone's webview allows much less memory.
export const PHONE_LIMITS: TerminalLimits = { images: 16_000_000, scrollback: 2_000_000 };
/// The most codepoints read for one cell; a longer grapheme is cut.
const GRAPHEME_CAP = 16;
/// DEC private mode 2027: a grapheme cluster, such as an emoji with a skin
/// tone, takes one cell. On by default, as in Ghostty itself.
const GRAPHEME_CLUSTER_MODE = 2027;

/// What a terminal asks of its host while it takes output.
export interface TerminalEffects {
  /// Bytes the terminal answers the program with, such as a cursor position
  /// or device attributes. Never what the user typed.
  reply(bytes: Uint8Array): void;
  bell(): void;
  titleChanged(): void;
}

export interface CellSize {
  width: number;
  height: number;
}

export type Dirty = "clean" | "partial" | "full";

type CursorShape = "bar" | "block" | "underline" | "hollow";

export interface Cursor {
  x: number;
  y: number;
  shape: CursorShape;
  blinking: boolean;
  /// On the right half of a wide character.
  wideTail: boolean;
}

/// Colours as 0xRRGGBB.
export interface Colors {
  background: number;
  foreground: number;
  cursor: number | null;
  palette: Uint32Array;
}

type ImageLayer = "below-background" | "below-text" | "above-text";

/// Where an image is drawn: from cell (col, row) of the view, which may be
/// above or left of it, moved by the offset, at width by height pixels of
/// the grid's cell size, showing the source rectangle of the image.
export interface ImagePlacement {
  id: number;
  /// Changes when the image's pixels do.
  generation: bigint;
  layer: ImageLayer;
  z: number;
  col: number;
  row: number;
  offsetX: number;
  offsetY: number;
  width: number;
  height: number;
  source: { x: number; y: number; width: number; height: number };
}

export interface ImagePixels {
  width: number;
  height: number;
  rgba: Uint8Array;
}

export type Underline = "none" | "single" | "double" | "curly" | "dotted" | "dashed";

/// Bits of RowCells.attrs.
export const Attr = {
  BOLD: 1 << 0,
  ITALIC: 1 << 1,
  FAINT: 1 << 2,
  BLINK: 1 << 3,
  STRIKETHROUGH: 1 << 4,
  OVERLINE: 1 << 5,
  SELECTED: 1 << 6,
  HYPERLINK: 1 << 7,
  /// The left half of a wide character; the cell after it is its tail.
  WIDE: 1 << 8,
  /// The right half of a wide character, drawn by the cell before it.
  WIDE_TAIL: 1 << 9,
  /// Underline style in three bits, as UNDERLINE_STYLES.
  UNDERLINE_SHIFT: 10,
  UNDERLINE_MASK: 0b111 << 10,
} as const;

/// Underline styles by their value in Attr's underline bits.
const UNDERLINE_STYLES: readonly Underline[] = ["none", "single", "double", "curly", "dotted", "dashed"];

/// One row of cells ready to draw, in arrays reused from row to row.
export class RowCells {
  /// The cell's first codepoint, 0 when empty.
  codepoint: Uint32Array;
  /// The text colour, after inverse video and hidden text.
  fg: Uint32Array;
  /// The background colour, the terminal's own when the cell sets none.
  bg: Uint32Array;
  underlineColor: Uint32Array;
  attrs: Uint16Array;
  /// Cells holding more than one codepoint, such as emoji sequences.
  graphemes = new Map<number, string>();
  cols = 0;

  constructor(cols: number) {
    this.codepoint = new Uint32Array(cols);
    this.fg = new Uint32Array(cols);
    this.bg = new Uint32Array(cols);
    this.underlineColor = new Uint32Array(cols);
    this.attrs = new Uint16Array(cols);
  }

  underline(x: number): Underline {
    return UNDERLINE_STYLES[(this.attrs[x] & Attr.UNDERLINE_MASK) >> Attr.UNDERLINE_SHIFT] ?? "none";
  }

  fit(cols: number) {
    if (this.codepoint.length < cols) {
      this.codepoint = new Uint32Array(cols);
      this.fg = new Uint32Array(cols);
      this.bg = new Uint32Array(cols);
      this.underlineColor = new Uint32Array(cols);
      this.attrs = new Uint16Array(cols);
    }
    this.cols = cols;
    this.graphemes.clear();
  }
}

/// The exports this wrapper calls. Handles and pointers are i32.
export interface Exports {
  memory: WebAssembly.Memory;
  __indirect_function_table: WebAssembly.Table;
  ghostty_wasm_alloc(len: number): number;
  ghostty_wasm_free(ptr: number, len: number): void;
  ghostty_terminal_new(allocator: number, out: number, cols: number, rows: number): number;
  ghostty_terminal_free(terminal: number): void;
  ghostty_terminal_set(terminal: number, option: number, value: number): number;
  ghostty_terminal_get(terminal: number, data: number, out: number): number;
  ghostty_terminal_vt_write(terminal: number, ptr: number, len: number): void;
  ghostty_terminal_resize(terminal: number, cols: number, rows: number, cellWidth: number, cellHeight: number): number;
  ghostty_terminal_scroll_viewport(terminal: number, behavior: number): number;
  ghostty_render_state_new(allocator: number, out: number): number;
  ghostty_render_state_free(state: number): void;
  ghostty_render_state_update(state: number, terminal: number): number;
  ghostty_render_state_get(state: number, data: number, out: number): number;
  ghostty_render_state_set(state: number, option: number, value: number): number;
  ghostty_render_state_row_iterator_new(allocator: number, out: number): number;
  ghostty_render_state_row_iterator_free(iterator: number): void;
  ghostty_render_state_row_iterator_next(iterator: number): number;
  ghostty_render_state_row_get(iterator: number, data: number, out: number): number;
  ghostty_render_state_row_set(iterator: number, option: number, value: number): number;
  ghostty_render_state_row_cells_new(allocator: number, out: number): number;
  ghostty_render_state_row_cells_free(cells: number): void;
  ghostty_render_state_row_cells_select(cells: number, x: number): number;
  ghostty_render_state_row_cells_get(cells: number, data: number, out: number): number;
  ghostty_free(allocator: number, ptr: number, len: number): void;
  ghostty_kitty_graphics_image(graphics: number, id: number): number;
  ghostty_kitty_graphics_image_get(image: number, data: number, out: number): number;
  ghostty_kitty_graphics_animation_tick(graphics: number, nowMs: bigint, next: number): number;
  ghostty_render_state_image_iterator_new(allocator: number, out: number): number;
  ghostty_render_state_image_iterator_free(iterator: number): void;
  ghostty_render_state_image_iterator_next(iterator: number): number;
  ghostty_render_state_image_get(iterator: number, data: number, out: number): number;
  ghostty_key_encoder_new(allocator: number, out: number): number;
  ghostty_key_encoder_free(encoder: number): void;
  ghostty_key_encoder_set(encoder: number, option: number, value: number): number;
  ghostty_key_encoder_set_from_terminal(encoder: number, terminal: number): number;
  ghostty_key_encoder_encode(encoder: number, event: number, buf: number, len: number, written: number): number;
  ghostty_key_event_new(allocator: number, out: number): number;
  ghostty_key_event_free(event: number): void;
  ghostty_key_event_set_action(event: number, action: number): number;
  ghostty_key_event_set_key(event: number, key: number): number;
  ghostty_key_event_set_mods(event: number, mods: number): number;
  ghostty_key_event_set_consumed_mods(event: number, mods: number): number;
  ghostty_key_event_set_utf8(event: number, str: number): number;
  ghostty_key_event_set_unshifted_codepoint(event: number, codepoint: number): number;
  ghostty_mouse_encoder_new(allocator: number, out: number): number;
  ghostty_mouse_encoder_free(encoder: number): void;
  ghostty_mouse_encoder_set(encoder: number, option: number, value: number): number;
  ghostty_mouse_encoder_set_from_terminal(encoder: number, terminal: number): number;
  ghostty_mouse_encoder_encode(encoder: number, event: number, buf: number, len: number, written: number): number;
  ghostty_mouse_event_new(allocator: number, out: number): number;
  ghostty_mouse_event_free(event: number): void;
  ghostty_mouse_event_set_action(event: number, action: number): number;
  ghostty_mouse_event_set_button(event: number, button: number): number;
  ghostty_mouse_event_clear_button(event: number): number;
  ghostty_mouse_event_set_mods(event: number, mods: number): number;
  ghostty_mouse_event_set_position(event: number, position: number): number;
  ghostty_paste_encode(data: number, len: number, bracketed: number, buf: number, cap: number, written: number): number;
  ghostty_focus_encode(event: number, buf: number, cap: number, written: number): number;
  ghostty_terminal_grid_ref(terminal: number, point: number, out: number): number;
  ghostty_terminal_select_all(terminal: number, out: number): number;
  ghostty_selection_gesture_new(allocator: number, out: number): number;
  ghostty_selection_gesture_free(gesture: number, terminal: number): void;
  ghostty_selection_gesture_reset(gesture: number, terminal: number): void;
  ghostty_selection_gesture_event_new(allocator: number, out: number, type: number): number;
  ghostty_selection_gesture_event_free(event: number): void;
  ghostty_selection_gesture_event_set(event: number, option: number, value: number): number;
  ghostty_selection_gesture_event(gesture: number, terminal: number, event: number, out: number): number;
  ghostty_formatter_terminal_new(allocator: number, out: number, terminal: number): number;
  ghostty_formatter_free(formatter: number): void;
  ghostty_formatter_set(formatter: number, option: number, value: number): number;
  ghostty_formatter_format_alloc(formatter: number, allocator: number, outPtr: number, outLen: number): number;
}

/// Loads the engine from its compiled module. One engine serves any number
/// of terminals.
export async function loadGhostty(module: WebAssembly.Module): Promise<Ghostty> {
  const instance = await WebAssembly.instantiate(module, {});
  return new Ghostty(instance.exports as unknown as Exports);
}

/// The engine's memory and the callbacks every terminal shares.
export class Ghostty {
  /// A scratch area for out-parameters, valid until the next call.
  private readonly scratch: number;
  private readonly terminals = new Map<number, Terminal>();
  private nextTerminal = 1;
  readonly callbacks: { writePty: number; bell: number; titleChanged: number; size: number };
  readonly x: Exports;

  constructor(x: Exports) {
    this.x = x;
    this.scratch = x.ghostty_wasm_alloc(1024);

    const by = (userdata: number) => this.terminals.get(userdata);
    this.callbacks = {
      writePty: this.callback(4, false, (_t: number, userdata: number, ptr: number, len: number) => {
        by(userdata)?.effects.reply(this.bytes(ptr, len).slice());
      }),
      bell: this.callback(2, false, (_t: number, userdata: number) => by(userdata)?.effects.bell()),
      titleChanged: this.callback(2, false, (_t: number, userdata: number) => by(userdata)?.effects.titleChanged()),
      size: this.callback(3, true, (_t: number, userdata: number, out: number) => (by(userdata)?.reportSize(out) ? 1 : 0)),
    };
  }

  /// The scratch area, cleared to `len` zero bytes.
  out(len: number): number {
    this.bytes(this.scratch, len).fill(0);
    return this.scratch;
  }

  view(): DataView {
    return new DataView(this.x.memory.buffer);
  }

  bytes(ptr: number, len: number): Uint8Array {
    return new Uint8Array(this.x.memory.buffer, ptr, len);
  }

  u32s(ptr: number, len: number): Uint32Array {
    return new Uint32Array(this.x.memory.buffer, ptr, len);
  }

  string(ptr: number): string {
    const view = this.view();
    const data = view.getUint32(ptr + GhosttyString.field.ptr, true);
    const len = view.getUint32(ptr + GhosttyString.field.len, true);
    return len === 0 ? "" : new TextDecoder().decode(this.bytes(data, len));
  }

  register(terminal: Terminal): number {
    const id = this.nextTerminal++;
    this.terminals.set(id, terminal);
    return id;
  }

  unregister(id: number) {
    this.terminals.delete(id);
  }

  /// Puts `fn` in the engine's function table, as a function taking
  /// `params` i32 arguments and returning an i32 when `returns`, and gives
  /// its index: the C function pointer.
  private callback(params: number, returns: boolean, fn: (...args: number[]) => unknown): number {
    const table = this.x.__indirect_function_table;
    const index = table.grow(1);
    table.set(index, trampoline(params, returns, fn));
    return index;
  }
}

/// A WebAssembly function calling `fn`, so the table can hold it: a module
/// that imports `fn` with the given signature and exports it again.
function trampoline(params: number, returns: boolean, fn: (...args: number[]) => unknown): WebAssembly.ExportValue {
  const I32 = 0x7f;
  const type = [0x01, 0x60, params, ...Array<number>(params).fill(I32), ...(returns ? [0x01, I32] : [0x00])];
  const imports = [0x01, 0x01, 0x65, 0x01, 0x66, 0x00, 0x00]; // "e" "f", a function of type 0
  const exports = [0x01, 0x01, 0x66, 0x00, 0x00]; // "f", function 0
  const section = (id: number, body: number[]) => [id, body.length, ...body];
  const bytes = new Uint8Array([
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00,
    ...section(0x01, type),
    ...section(0x02, imports),
    ...section(0x07, exports),
  ]);
  const instance = new WebAssembly.Instance(new WebAssembly.Module(bytes), { e: { f: fn } });
  const exported = instance.exports.f;
  if (exported === undefined) throw new Error("trampoline module exported nothing");
  return exported;
}

function check(result: number, what: string) {
  if (result !== GhosttyResult.SUCCESS) throw new Error(`${what} failed: ${resultName(result)}`);
}

function resultName(result: number): string {
  const found = Object.entries(GhosttyResult).find(([, value]) => value === result);
  return found ? found[0] : String(result);
}

/// A terminal: program output in, screen state out.
export class Terminal {
  readonly handle: number;
  private readonly id: number;
  /// Where output is copied in before Ghostty reads it.
  private input = 0;
  private inputLen = 0;
  private freed = false;
  /// The grid and cell size of the last resize, for size queries.
  private grid: { cols: number; rows: number; cell: CellSize | null };
  private readonly g: Ghostty;
  readonly effects: TerminalEffects;

  constructor(g: Ghostty, cols: number, rows: number, effects: TerminalEffects, limits: TerminalLimits = DESKTOP_LIMITS) {
    this.g = g;
    this.effects = effects;
    this.grid = { cols, rows, cell: null };
    const x = g.x;
    const out = g.out(4);
    check(x.ghostty_terminal_new(0, out, cols, rows), "ghostty_terminal_new");
    this.handle = g.view().getUint32(out, true);
    this.id = g.register(this);

    check(x.ghostty_terminal_set(this.handle, GhosttyTerminalOption.USERDATA, this.id), "setting userdata");
    check(x.ghostty_terminal_set(this.handle, GhosttyTerminalOption.WRITE_PTY, g.callbacks.writePty), "setting write_pty");
    check(x.ghostty_terminal_set(this.handle, GhosttyTerminalOption.BELL, g.callbacks.bell), "setting bell");
    check(x.ghostty_terminal_set(this.handle, GhosttyTerminalOption.TITLE_CHANGED, g.callbacks.titleChanged), "setting title_changed");
    // Programs that size pictures to cells (yazi) ask for the cell size in
    // pixels, which the pseudo-terminal does not carry.
    check(x.ghostty_terminal_set(this.handle, GhosttyTerminalOption.SIZE, g.callbacks.size), "setting size");

    const limit = g.out(8);
    g.view().setBigUint64(limit, BigInt(limits.images), true);
    check(x.ghostty_terminal_set(this.handle, GhosttyTerminalOption.KITTY_IMAGE_STORAGE_LIMIT, limit), "setting the image limit");

    const scrollback = g.out(4);
    g.view().setUint32(scrollback, limits.scrollback, true);
    check(x.ghostty_terminal_set(this.handle, GhosttyTerminalOption.SCROLLBACK_MAX_BYTES, scrollback), "setting the scrollback");

    // The default as well as the current value, so a reset keeps it.
    this.setMode(GhosttyTerminalOption.MODE_DEFAULT, GRAPHEME_CLUSTER_MODE, true);
    this.setMode(GhosttyTerminalOption.MODE, GRAPHEME_CLUSTER_MODE, true);
  }

  /// Takes program output.
  write(data: Uint8Array) {
    if (data.length === 0) return;

    if (this.inputLen < data.length) {
      if (this.input !== 0) this.g.x.ghostty_wasm_free(this.input, this.inputLen);
      this.inputLen = Math.max(data.length, 64 * 1024);
      this.input = this.g.x.ghostty_wasm_alloc(this.inputLen);
      if (this.input === 0) throw new Error("out of memory for terminal output");
    }

    this.g.bytes(this.input, data.length).set(data);
    this.g.x.ghostty_terminal_vt_write(this.handle, this.input, data.length);
  }

  resize(cols: number, rows: number, cell: CellSize) {
    this.grid = { cols, rows, cell };
    check(this.g.x.ghostty_terminal_resize(this.handle, cols, rows, cell.width, cell.height), "ghostty_terminal_resize");
  }

  /// Fills Ghostty's size report at `out`; false until the cell size is
  /// known, so a query goes unanswered rather than wrong.
  reportSize(out: number): boolean {
    const { cols, rows, cell } = this.grid;
    if (cell === null) return false;

    const view = this.g.view();
    const f = GhosttySizeReportSize.field;
    view.setUint16(out + f.rows, rows, true);
    view.setUint16(out + f.columns, cols, true);
    view.setUint32(out + f.cell_width, cell.width, true);
    view.setUint32(out + f.cell_height, cell.height, true);
    return true;
  }

  /// Sets the default colours programs see and draw with, as 0xRRGGBB.
  /// `palette` replaces some of the 256 palette entries by index.
  setColors(colors: { foreground: number; background: number; cursor: number; palette: ReadonlyMap<number, number> }) {
    const g = this.g;
    const x = g.x;
    const one = (option: GhosttyTerminalOption, color: number) => {
      const out = g.out(3);
      g.bytes(out, 3).set([(color >> 16) & 0xff, (color >> 8) & 0xff, color & 0xff]);
      check(x.ghostty_terminal_set(this.handle, option, out), "setting a colour");
    };
    one(GhosttyTerminalOption.COLOR_FOREGROUND, colors.foreground);
    one(GhosttyTerminalOption.COLOR_BACKGROUND, colors.background);
    one(GhosttyTerminalOption.COLOR_CURSOR, colors.cursor);

    const palette = g.out(256 * 3);
    check(x.ghostty_terminal_get(this.handle, GhosttyTerminalData.COLOR_PALETTE_DEFAULT, palette), "reading the palette");
    const bytes = g.bytes(palette, 256 * 3);
    for (const [index, color] of colors.palette) bytes.set([(color >> 16) & 0xff, (color >> 8) & 0xff, color & 0xff], index * 3);
    check(x.ghostty_terminal_set(this.handle, GhosttyTerminalOption.COLOR_PALETTE, palette), "setting the palette");
  }

  /// Scrolls the view by `rows`, up when negative.
  scrollBy(rows: number) {
    this.scroll(GhosttyTerminalScrollViewportTag.DELTA, rows);
  }

  scrollToBottom() {
    this.scroll(GhosttyTerminalScrollViewportTag.BOTTOM, 0);
  }

  title(): string {
    const out = this.g.out(GhosttyString.size);
    return this.g.x.ghostty_terminal_get(this.handle, GhosttyTerminalData.TITLE, out) === GhosttyResult.SUCCESS
      ? this.g.string(out)
      : "";
  }

  size(): { cols: number; rows: number } {
    const out = this.g.out(2);
    check(this.g.x.ghostty_terminal_get(this.handle, GhosttyTerminalData.COLS, out), "reading cols");
    const cols = this.g.view().getUint16(out, true);
    check(this.g.x.ghostty_terminal_get(this.handle, GhosttyTerminalData.ROWS, out), "reading rows");
    return { cols, rows: this.g.view().getUint16(out, true) };
  }

  /// An image's pixels as RGBA, or null when the terminal no longer has it.
  /// Read them again when a placement's generation changes.
  imagePixels(id: number): ImagePixels | null {
    const g = this.g;
    const x = g.x;
    const graphics = this.graphics();
    if (graphics === 0) return null;

    const image = x.ghostty_kitty_graphics_image(graphics, id);
    if (image === 0) return null;

    const get = (key: GhosttyKittyGraphicsImageData, len: number) => {
      const out = g.out(len);
      check(x.ghostty_kitty_graphics_image_get(image, key, out), "reading an image");
      return out;
    };
    const width = g.view().getUint32(get(GhosttyKittyGraphicsImageData.WIDTH, 4), true);
    const height = g.view().getUint32(get(GhosttyKittyGraphicsImageData.HEIGHT, 4), true);
    const format = g.view().getInt32(get(GhosttyKittyGraphicsImageData.FORMAT, 4), true);
    const pixels = get(GhosttyKittyGraphicsImageData.PIXELS, GhosttyString.size);
    const ptr = g.view().getUint32(pixels + GhosttyString.field.ptr, true);
    const len = g.view().getUint32(pixels + GhosttyString.field.len, true);

    const rgba = toRgba(g.bytes(ptr, len), format, width * height);
    return rgba === null ? null : { width, height, rgba };
  }

  /// Advances image animations to `nowMs` on a steady clock, and gives when
  /// the next frame is due, or null when none plays.
  tickAnimations(nowMs: number): number | null {
    const graphics = this.graphics();
    if (graphics === 0) return null;

    const out = this.g.out(8);
    const result = this.g.x.ghostty_kitty_graphics_animation_tick(graphics, BigInt(Math.floor(nowMs)), out);
    return result === GhosttyResult.SUCCESS ? Number(this.g.view().getBigUint64(out, true)) : null;
  }

  free() {
    if (this.freed) return;
    this.freed = true;
    this.g.unregister(this.id);
    if (this.input !== 0) this.g.x.ghostty_wasm_free(this.input, this.inputLen);
    this.g.x.ghostty_terminal_free(this.handle);
  }

  private graphics(): number {
    const out = this.g.out(4);
    if (this.g.x.ghostty_terminal_get(this.handle, GhosttyTerminalData.KITTY_GRAPHICS, out) !== GhosttyResult.SUCCESS) return 0;
    return this.g.view().getUint32(out, true);
  }

  /// Sets DEC private mode `mode` through `option`, MODE or MODE_DEFAULT.
  private setMode(option: GhosttyTerminalOption, mode: number, on: boolean) {
    const config = this.g.out(GhosttyTerminalModeConfig.size);
    const view = this.g.view();
    view.setUint16(config + GhosttyTerminalModeConfig.field.mode, mode & 0x7fff, true);
    view.setUint8(config + GhosttyTerminalModeConfig.field.value, on ? 1 : 0);
    check(this.g.x.ghostty_terminal_set(this.handle, option, config), `setting mode ${mode}`);
  }

  private scroll(tag: GhosttyTerminalScrollViewportTag, delta: number) {
    // Passed by value, which wasm32's C ABI passes as a pointer to a copy.
    const behavior = this.g.out(GhosttyTerminalScrollViewport.size);
    const view = this.g.view();
    view.setInt32(behavior + GhosttyTerminalScrollViewport.field.tag, tag, true);
    view.setInt32(behavior + GhosttyTerminalScrollViewport.field.value, delta, true);
    check(this.g.x.ghostty_terminal_scroll_viewport(this.handle, behavior), "ghostty_terminal_scroll_viewport");
  }
}

/// What to draw: a terminal's screen, read after each update.
export class RenderState {
  private readonly handle: number;
  private readonly rows: number;
  private readonly cells: number;
  /// Memory for grapheme codepoints.
  private readonly graphemes: number;
  private readonly images: number;
  private readonly palette = new Uint32Array(256);
  private colors: Colors = { background: 0, foreground: 0, cursor: null, palette: this.palette };
  /// Styles of the current row by style id; ids are per page, so per row.
  private readonly styles = new Map<number, StyleRead>();
  private readonly g: Ghostty;

  constructor(g: Ghostty) {
    this.g = g;
    const x = g.x;
    const out = g.out(4);
    check(x.ghostty_render_state_new(0, out), "ghostty_render_state_new");
    this.handle = g.view().getUint32(out, true);
    check(x.ghostty_render_state_row_iterator_new(0, out), "ghostty_render_state_row_iterator_new");
    this.rows = g.view().getUint32(out, true);
    check(x.ghostty_render_state_row_cells_new(0, out), "ghostty_render_state_row_cells_new");
    this.cells = g.view().getUint32(out, true);
    check(x.ghostty_render_state_image_iterator_new(0, out), "ghostty_render_state_image_iterator_new");
    this.images = g.view().getUint32(out, true);
    this.graphemes = x.ghostty_wasm_alloc(GRAPHEME_CAP * 4);
  }

  /// Reads the terminal's current state, and says how much changed since
  /// the last `clean`.
  update(terminal: Terminal): Dirty {
    check(this.g.x.ghostty_render_state_update(this.handle, terminal.handle), "ghostty_render_state_update");
    this.colors = this.readColors();

    const out = this.g.out(4);
    check(this.g.x.ghostty_render_state_get(this.handle, GhosttyRenderStateData.DIRTY, out), "reading dirty");
    switch (this.g.view().getInt32(out, true)) {
      case GhosttyRenderStateDirty.FALSE:
        return "clean";
      case GhosttyRenderStateDirty.PARTIAL:
        return "partial";
      default:
        return "full";
    }
  }

  currentColors(): Colors {
    return this.colors;
  }

  cursor(): Cursor | null {
    const g = this.g;
    const out = g.out(GhosttyRenderStateCursor.size);
    const view = g.view();
    view.setUint32(out, GhosttyRenderStateCursor.size, true);
    check(g.x.ghostty_render_state_get(this.handle, GhosttyRenderStateData.CURSOR, out), "reading the cursor");

    const f = GhosttyRenderStateCursor.field;
    if (view.getUint8(out + f.visible) === 0 || view.getUint8(out + f.viewport_has_value) === 0) return null;

    return {
      x: view.getUint16(out + f.viewport_x, true),
      y: view.getUint16(out + f.viewport_y, true),
      shape: cursorShape(view.getInt32(out + f.visual_style, true)),
      blinking: view.getUint8(out + f.blinking) !== 0,
      wideTail: view.getUint8(out + f.wide_tail) !== 0,
    };
  }

  /// Calls `draw` for each row that changed, or every row when `all`, with
  /// its cells in `row`, then marks them drawn.
  forEachRow(all: boolean, row: RowCells, draw: (y: number, row: RowCells) => void) {
    const g = this.g;
    const x = g.x;
    let out = g.out(4);
    g.view().setUint32(out, this.rows, true);
    check(x.ghostty_render_state_get(this.handle, GhosttyRenderStateData.ROW_ITERATOR, out), "reading rows");

    for (let y = 0; x.ghostty_render_state_row_iterator_next(this.rows) !== 0; y++) {
      out = g.out(4);
      check(x.ghostty_render_state_row_get(this.rows, GhosttyRenderStateRowData.DIRTY, out), "reading a row's dirty flag");
      if (!all && g.view().getUint8(out) === 0) continue;

      this.readRow(row);
      draw(y, row);

      out = g.out(1); // false
      check(x.ghostty_render_state_row_set(this.rows, GhosttyRenderStateRowOption.DIRTY, out), "marking a row drawn");
    }
  }

  /// The images to draw, in drawing order: by layer, then z-index.
  placements(): ImagePlacement[] {
    const g = this.g;
    const x = g.x;
    const out = g.out(4);
    g.view().setUint32(out, this.images, true);
    check(x.ghostty_render_state_get(this.handle, GhosttyRenderStateData.IMAGE_ITERATOR, out), "reading images");

    const placements: ImagePlacement[] = [];
    while (x.ghostty_render_state_image_iterator_next(this.images) !== 0) {
      const u32 = (key: GhosttyRenderStateImageData) => {
        const at = g.out(4);
        check(x.ghostty_render_state_image_get(this.images, key, at), "reading an image placement");
        return g.view().getUint32(at, true);
      };
      const i32 = (key: GhosttyRenderStateImageData) => u32(key) | 0;
      const generation = g.out(8);
      check(x.ghostty_render_state_image_get(this.images, GhosttyRenderStateImageData.IMAGE_GENERATION, generation), "reading an image generation");
      const D = GhosttyRenderStateImageData;

      placements.push({
        id: u32(D.IMAGE_ID),
        generation: g.view().getBigUint64(generation, true),
        layer: imageLayer(i32(D.LAYER)),
        z: i32(D.Z),
        col: i32(D.VIEWPORT_COL),
        row: i32(D.VIEWPORT_ROW),
        offsetX: u32(D.CELL_OFFSET_X),
        offsetY: u32(D.CELL_OFFSET_Y),
        width: u32(D.PIXEL_WIDTH),
        height: u32(D.PIXEL_HEIGHT),
        source: { x: u32(D.SOURCE_X), y: u32(D.SOURCE_Y), width: u32(D.SOURCE_WIDTH), height: u32(D.SOURCE_HEIGHT) },
      });
    }

    const order: Record<ImageLayer, number> = { "below-background": 0, "below-text": 1, "above-text": 2 };
    return placements.sort((a, b) => order[a.layer] - order[b.layer] || a.z - b.z);
  }

  /// Marks the whole frame drawn.
  clean() {
    const out = this.g.out(4); // GhosttyRenderStateDirty.FALSE
    check(this.g.x.ghostty_render_state_set(this.handle, GhosttyRenderStateOption.DIRTY, out), "marking the frame drawn");
  }

  free() {
    const x = this.g.x;
    x.ghostty_wasm_free(this.graphemes, GRAPHEME_CAP * 4);
    x.ghostty_render_state_image_iterator_free(this.images);
    x.ghostty_render_state_row_cells_free(this.cells);
    x.ghostty_render_state_row_iterator_free(this.rows);
    x.ghostty_render_state_free(this.handle);
  }

  private readColors(): Colors {
    const g = this.g;
    const out = g.out(GhosttyRenderStateColors.size);
    const view = g.view();
    view.setUint32(out, GhosttyRenderStateColors.size, true);
    check(g.x.ghostty_render_state_get(this.handle, GhosttyRenderStateData.COLORS, out), "reading colours");

    const f = GhosttyRenderStateColors.field;
    const bytes = g.bytes(out, GhosttyRenderStateColors.size);
    for (let i = 0; i < 256; i++) this.palette[i] = rgbAt(bytes, f.palette + i * 3);

    return {
      background: rgbAt(bytes, f.background),
      foreground: rgbAt(bytes, f.foreground),
      cursor: view.getUint8(out + f.cursor_has_value) !== 0 ? rgbAt(bytes, f.cursor) : null,
      palette: this.palette,
    };
  }

  /// Decodes the iterator's current row into `row`, reading the packed
  /// cells straight from the engine's memory and asking it only for styles
  /// and multi-codepoint graphemes.
  private readRow(row: RowCells) {
    const g = this.g;
    const x = g.x;
    let out = g.out(4);
    g.view().setUint32(out, this.cells, true);
    check(x.ghostty_render_state_row_get(this.rows, GhosttyRenderStateRowData.CELLS, out), "reading a row's cells");

    out = g.out(8);
    check(x.ghostty_render_state_row_get(this.rows, GhosttyRenderStateRowData.CELLS_RAW, out), "reading raw cells");
    const view = g.view();
    const ptr = view.getUint32(out, true);
    const cols = view.getUint32(out + 4, true);
    row.fit(cols);

    const selection = this.selection();
    const colors = this.colors;
    const raw = g.u32s(ptr, cols * 2);
    this.styles.clear();

    for (let i = 0; i < cols; i++) {
      const lo = raw[i * 2];
      const hi = raw[i * 2 + 1];
      const tag = bits(lo, hi, GhosttyCell.bits.content_tag);
      const content = bits(lo, hi, GhosttyCell.bits.content);
      const styleId = bits(lo, hi, GhosttyCell.bits.style_id);
      const wide = bits(lo, hi, GhosttyCell.bits.wide);

      const style = styleId === 0 ? DEFAULT_STYLE : this.style(styleId, i);
      let fg = resolve(style.fg, colors, colors.foreground);
      let bg = resolve(style.bg, colors, colors.background);
      let codepoint = 0;

      switch (tag) {
        case GhosttyCellContentTag.CODEPOINT:
          codepoint = sub(content, GhosttyCell.arms.content.CODEPOINT.codepoint);
          break;
        case GhosttyCellContentTag.CODEPOINT_GRAPHEME:
          codepoint = sub(content, GhosttyCell.arms.content.CODEPOINT_GRAPHEME.codepoint);
          row.graphemes.set(i, this.grapheme(i));
          break;
        case GhosttyCellContentTag.BG_COLOR_PALETTE:
          bg = colors.palette[sub(content, GhosttyCell.arms.content.BG_COLOR_PALETTE.index)];
          break;
        case GhosttyCellContentTag.BG_COLOR_RGB: {
          const arm = GhosttyCell.arms.content.BG_COLOR_RGB;
          bg = (sub(content, arm.r) << 16) | (sub(content, arm.g) << 8) | sub(content, arm.b);
          break;
        }
      }

      if (style.inverse) [fg, bg] = [bg, fg];
      if (style.invisible) fg = bg;

      let attrs = style.attrs;
      if (wide === GhosttyCellWide.WIDE) attrs |= Attr.WIDE;
      if (wide === GhosttyCellWide.SPACER_TAIL) attrs |= Attr.WIDE_TAIL;
      if (bits(lo, hi, GhosttyCell.bits.hyperlink) !== 0) attrs |= Attr.HYPERLINK;
      if (selection !== null && i >= selection.start && i <= selection.end) attrs |= Attr.SELECTED;

      row.codepoint[i] = codepoint;
      row.fg[i] = fg;
      row.bg[i] = bg;
      row.underlineColor[i] = style.underline.tag === GhosttyStyleColorTag.NONE ? fg : resolve(style.underline, colors, fg);
      row.attrs[i] = attrs;
    }
  }

  private selection(): { start: number; end: number } | null {
    const g = this.g;
    const out = g.out(GhosttyRenderStateRowSelection.size);
    g.view().setUint32(out, GhosttyRenderStateRowSelection.size, true);
    if (g.x.ghostty_render_state_row_get(this.rows, GhosttyRenderStateRowData.SELECTION, out) !== GhosttyResult.SUCCESS) return null;

    const view = g.view();
    const f = GhosttyRenderStateRowSelection.field;
    return { start: view.getUint16(out + f.start_x, true), end: view.getUint16(out + f.end_x, true) };
  }

  private style(id: number, x: number): StyleRead {
    const known = this.styles.get(id);
    if (known) return known;

    const g = this.g;
    check(g.x.ghostty_render_state_row_cells_select(this.cells, x), "selecting a cell");
    const out = g.out(GhosttyStyle.size);
    check(g.x.ghostty_render_state_row_cells_get(this.cells, GhosttyRenderStateRowCellsData.STYLE, out), "reading a style");

    const style = readStyle(g.bytes(out, GhosttyStyle.size));
    this.styles.set(id, style);
    return style;
  }

  private grapheme(x: number): string {
    const g = this.g;
    check(g.x.ghostty_render_state_row_cells_select(this.cells, x), "selecting a cell");
    const out = g.out(GhosttyCodepointBuffer.size);
    const view = g.view();
    view.setUint32(out + GhosttyCodepointBuffer.field.ptr, this.graphemes, true);
    view.setUint32(out + GhosttyCodepointBuffer.field.cap, GRAPHEME_CAP, true);
    const result = g.x.ghostty_render_state_row_cells_get(this.cells, GhosttyRenderStateRowCellsData.GRAPHEMES_BUF, out);
    if (result !== GhosttyResult.SUCCESS && result !== GhosttyResult.OUT_OF_SPACE) return "";

    const len = Math.min(g.view().getUint32(out + GhosttyCodepointBuffer.field.len, true), GRAPHEME_CAP);
    return String.fromCodePoint(...g.u32s(this.graphemes, len));
  }
}

interface ColorRead {
  tag: number;
  /// The palette index or 0xRRGGBB, as the tag says.
  value: number;
}

interface StyleRead {
  fg: ColorRead;
  bg: ColorRead;
  underline: ColorRead;
  inverse: boolean;
  invisible: boolean;
  /// Attr bits from the style.
  attrs: number;
}

const NO_COLOR: ColorRead = { tag: GhosttyStyleColorTag.NONE, value: 0 };
const DEFAULT_STYLE: StyleRead = { fg: NO_COLOR, bg: NO_COLOR, underline: NO_COLOR, inverse: false, invisible: false, attrs: 0 };

function readStyle(bytes: Uint8Array): StyleRead {
  const color = (at: number): ColorRead => {
    const tag = bytes[at + GhosttyStyleColor.field.tag];
    const value = at + GhosttyStyleColor.field.value;
    return tag === GhosttyStyleColorTag.RGB ? { tag, value: rgbAt(bytes, value) } : { tag, value: bytes[value] };
  };
  const flags = bytes[GhosttyStyle.field.flags] | (bytes[GhosttyStyle.field.flags + 1] << 8);
  const flag = ([lsb, width]: readonly [number, number]) => (flags >>> lsb) & ((1 << width) - 1);
  const b = GhosttyStyleFlags.bits;

  const underline = flag(b.underline);
  let attrs = 0;
  if (flag(b.bold)) attrs |= Attr.BOLD;
  if (flag(b.italic)) attrs |= Attr.ITALIC;
  if (flag(b.faint)) attrs |= Attr.FAINT;
  if (flag(b.blink)) attrs |= Attr.BLINK;
  if (flag(b.strikethrough)) attrs |= Attr.STRIKETHROUGH;
  if (flag(b.overline)) attrs |= Attr.OVERLINE;
  if (underline <= GhosttySgrUnderline.DASHED) attrs |= underline << Attr.UNDERLINE_SHIFT;

  return {
    fg: color(GhosttyStyle.field.fg_color),
    bg: color(GhosttyStyle.field.bg_color),
    underline: color(GhosttyStyle.field.underline_color),
    inverse: flag(b.inverse) !== 0,
    invisible: flag(b.invisible) !== 0,
    attrs,
  };
}

function resolve(color: ColorRead, colors: Colors, fallback: number): number {
  switch (color.tag) {
    case GhosttyStyleColorTag.RGB:
      return color.value;
    case GhosttyStyleColorTag.PALETTE:
      return colors.palette[color.value];
    default:
      return fallback;
  }
}

function imageLayer(layer: number): ImageLayer {
  switch (layer) {
    case GhosttyImageLayer.BELOW_BACKGROUND:
      return "below-background";
    case GhosttyImageLayer.BELOW_TEXT:
      return "below-text";
    default:
      return "above-text";
  }
}

/// `pixels` of an image stored as `format`, as RGBA; null for a format the
/// engine does not store decoded.
function toRgba(data: Uint8Array, format: number, pixels: number): Uint8Array | null {
  const rgba = new Uint8Array(pixels * 4);

  switch (format) {
    case GhosttyKittyImageFormat.RGBA:
      rgba.set(data.subarray(0, pixels * 4));
      return rgba;
    case GhosttyKittyImageFormat.RGB:
      for (let i = 0; i < pixels; i++) {
        rgba[i * 4] = data[i * 3];
        rgba[i * 4 + 1] = data[i * 3 + 1];
        rgba[i * 4 + 2] = data[i * 3 + 2];
        rgba[i * 4 + 3] = 255;
      }
      return rgba;
    case GhosttyKittyImageFormat.GRAY_ALPHA:
      for (let i = 0; i < pixels; i++) {
        rgba.fill(data[i * 2], i * 4, i * 4 + 3);
        rgba[i * 4 + 3] = data[i * 2 + 1];
      }
      return rgba;
    case GhosttyKittyImageFormat.GRAY:
      for (let i = 0; i < pixels; i++) {
        rgba.fill(data[i], i * 4, i * 4 + 3);
        rgba[i * 4 + 3] = 255;
      }
      return rgba;
    default:
      return null;
  }
}

function cursorShape(style: number): CursorShape {
  switch (style) {
    case GhosttyCursorStyle.BAR:
      return "bar";
    case GhosttyCursorStyle.UNDERLINE:
      return "underline";
    case GhosttyCursorStyle.BLOCK_HOLLOW:
      return "hollow";
    default:
      return "block";
  }
}

function rgbAt(bytes: Uint8Array, at: number): number {
  return (bytes[at] << 16) | (bytes[at + 1] << 8) | bytes[at + 2];
}

/// Bit field [lsb, width] of a 64-bit value held as two 32-bit halves.
function bits(lo: number, hi: number, [lsb, width]: readonly [number, number]): number {
  const mask = width >= 32 ? 0xffffffff : (1 << width) - 1;
  if (lsb >= 32) return (hi >>> (lsb - 32)) & mask;
  if (lsb + width <= 32) return (lo >>> lsb) & mask;
  return ((lo >>> lsb) | (hi << (32 - lsb))) & mask;
}

/// Bit field [lsb, width] of a value of at most 32 bits.
function sub(value: number, [lsb, width]: readonly [number, number]): number {
  return (value >>> lsb) & ((1 << width) - 1);
}
