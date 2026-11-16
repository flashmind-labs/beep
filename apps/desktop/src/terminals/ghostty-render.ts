// Draws a Ghostty terminal with WebGL2: cell backgrounds, glyphs from an
// atlas, underlines and other lines, Kitty images in their layers, the
// selection and the cursor. Each row's instances are rebuilt only when Ghostty
// says the row changed, and uploaded alone.
//
// Everything is in device pixels: the grid's cell size is what the terminal
// reports to programs, so images are as sharp as the screen.
import { Attr, RenderState, RowCells, type CellSize, type Cursor, type ImagePlacement, type Terminal } from "./ghostty.ts";

export interface FontSpec {
  family: string;
  /// In CSS pixels.
  size: number;
  lineHeight: number;
}

/// Colours as 0xRRGGBB; the selection is drawn over cells at its alpha.
export interface Theme {
  selection: number;
  selectionAlpha: number;
}

/// How the cursor is shown this frame.
export interface CursorLook {
  /// In the off half of a blink; a steady cursor stays drawn.
  blinkedOut: boolean;
  /// Keys go to this screen; else a block cursor is drawn hollow.
  focused: boolean;
}

/// Floats per instance: rect (x, y, w, h), colour (r, g, b, a), kind.
const RECT_FLOATS = 9;
/// Floats per glyph: rect, atlas rect (u, v, w, h), colour (r, g, b, coloured).
const GLYPH_FLOATS = 12;
/// Underline (two lines when double), strikethrough and overline.
const DECORATIONS_PER_CELL = 4;
const ATLAS_SIZE = 2048;
/// The character kitty's Unicode placeholders put in each cell of an image.
const KITTY_PLACEHOLDER = 0x10eeee;

/// What a rect instance draws, read by the fragment shader.
const Kind = { SOLID: 0, CURLY: 1, DOTTED: 2, DASHED: 3, HOLLOW: 4 } as const;

const RECT_VERTEX = `#version 300 es
in vec2 a_corner;
in vec4 a_rect;
in vec4 a_color;
in float a_kind;
uniform vec2 u_view;
out vec2 v_local;
out vec2 v_size;
out vec4 v_color;
flat out int v_kind;
void main() {
  vec2 p = a_rect.xy + a_corner * a_rect.zw;
  v_local = a_corner * a_rect.zw;
  v_size = a_rect.zw;
  v_color = a_color;
  v_kind = int(a_kind);
  vec2 clip = p / u_view * 2.0 - 1.0;
  gl_Position = vec4(clip.x, -clip.y, 0.0, 1.0);
}`;

const RECT_FRAGMENT = `#version 300 es
precision highp float;
in vec2 v_local;
in vec2 v_size;
in vec4 v_color;
flat in int v_kind;
out vec4 color;
void main() {
  float a = 1.0;
  if (v_kind == 1) {
    // Curly: a sine wave across the cell, as thick as the line height allows.
    float mid = v_size.y * 0.5;
    float wave = mid + sin(v_local.x / v_size.y * 3.14159) * (mid - 1.0);
    a = clamp(1.0 - abs(v_local.y - wave) + 0.5, 0.0, 1.0);
  } else if (v_kind == 2) {
    a = mod(floor(v_local.x / max(v_size.y, 1.0)), 2.0) < 1.0 ? 1.0 : 0.0;
  } else if (v_kind == 3) {
    a = mod(floor(v_local.x / (max(v_size.y, 1.0) * 3.0)), 2.0) < 1.0 ? 1.0 : 0.0;
  } else if (v_kind == 4) {
    // Hollow: a one pixel frame.
    float edge = min(min(v_local.x, v_size.x - v_local.x), min(v_local.y, v_size.y - v_local.y));
    a = edge < 1.0 ? 1.0 : 0.0;
  }
  color = vec4(v_color.rgb * v_color.a * a, v_color.a * a);
}`;

const GLYPH_VERTEX = `#version 300 es
in vec2 a_corner;
in vec4 a_rect;
in vec4 a_uv;
in vec4 a_color;
uniform vec2 u_view;
out vec2 v_uv;
out vec4 v_color;
void main() {
  vec2 p = a_rect.xy + a_corner * a_rect.zw;
  v_uv = a_uv.xy + a_corner * a_uv.zw;
  v_color = a_color;
  vec2 clip = p / u_view * 2.0 - 1.0;
  gl_Position = vec4(clip.x, -clip.y, 0.0, 1.0);
}`;

const GLYPH_FRAGMENT = `#version 300 es
precision highp float;
uniform sampler2D u_atlas;
in vec2 v_uv;
in vec4 v_color;
out vec4 color;
void main() {
  vec4 t = texture(u_atlas, v_uv);
  // Coloured glyphs (emoji) keep their colours; text takes its own colour
  // through the glyph's coverage. The atlas is premultiplied.
  color = v_color.a > 0.5 ? t : vec4(v_color.rgb * t.a, t.a);
}`;

/// Draws one terminal into one canvas.
export class GridRenderer {
  readonly canvas: HTMLCanvasElement;
  private readonly gl: WebGL2RenderingContext;
  private readonly font: FontSpec;
  /// Changed when the skin changes; the next full redraw shows it.
  theme: Theme;
  private dpr = 1;
  private cell: CellSize = { width: 1, height: 1 };
  private baseline = 0;
  private cols = 0;
  private rows = 0;

  private readonly rectProgram: Program;
  private readonly glyphProgram: Program;
  private readonly quad: WebGLBuffer;
  private readonly bg: InstanceBuffer;
  private readonly glyphs: InstanceBuffer;
  private readonly decorations: InstanceBuffer;
  /// The cursor, and the glyph under a block cursor drawn again over it.
  private readonly cursorRect: InstanceBuffer;
  private readonly cursorGlyph: InstanceBuffer;
  /// One image placement at a time.
  private readonly image: InstanceBuffer;
  private readonly atlas: GlyphAtlas;
  private readonly images = new ImageTextures();
  private readonly row = new RowCells(0);

  constructor(canvas: HTMLCanvasElement, font: FontSpec, theme: Theme) {
    const gl = canvas.getContext("webgl2", { alpha: false, antialias: false, premultipliedAlpha: true, preserveDrawingBuffer: false });
    if (!gl) throw new Error("WebGL2 is unavailable");

    this.canvas = canvas;
    this.gl = gl;
    this.font = font;
    this.theme = theme;
    this.rectProgram = program(gl, RECT_VERTEX, RECT_FRAGMENT);
    this.glyphProgram = program(gl, GLYPH_VERTEX, GLYPH_FRAGMENT);
    this.quad = buffer(gl, new Float32Array([0, 0, 1, 0, 0, 1, 1, 1]));
    this.bg = new InstanceBuffer(gl, RECT_FLOATS);
    this.glyphs = new InstanceBuffer(gl, GLYPH_FLOATS);
    this.decorations = new InstanceBuffer(gl, RECT_FLOATS);
    this.cursorRect = new InstanceBuffer(gl, RECT_FLOATS);
    this.cursorGlyph = new InstanceBuffer(gl, GLYPH_FLOATS);
    this.image = new InstanceBuffer(gl, GLYPH_FLOATS);
    this.atlas = new GlyphAtlas(gl);

    gl.enable(gl.BLEND);
    gl.blendFunc(gl.ONE, gl.ONE_MINUS_SRC_ALPHA);
  }

  /// The cell size in device pixels for `dpr`, measured from the font.
  measure(dpr: number): CellSize {
    if (dpr !== this.dpr || this.cell.width === 1) {
      this.dpr = dpr;
      const m = this.atlas.setFont(this.font, dpr);
      this.cell = { width: m.width, height: m.height };
      this.baseline = m.baseline;
    }
    return this.cell;
  }

  /// Sizes the canvas for a grid; everything is drawn again next frame.
  resize(cols: number, rows: number) {
    this.cols = cols;
    this.rows = rows;
    this.canvas.width = cols * this.cell.width;
    this.canvas.height = rows * this.cell.height;
    this.canvas.style.width = `${this.canvas.width / this.dpr}px`;
    this.canvas.style.height = `${this.canvas.height / this.dpr}px`;
    this.bg.reserve(rows * cols);
    this.glyphs.reserve(rows * cols);
    this.decorations.reserve(rows * cols * DECORATIONS_PER_CELL);
    this.cursorRect.reserve(1);
    this.cursorGlyph.reserve(1);
    this.image.reserve(1);
  }

  /// Draws the terminal: only changed rows unless `full`.
  draw(state: RenderState, term: Terminal, full: boolean, look: CursorLook) {
    const gl = this.gl;
    const colors = state.currentColors();
    const cols = this.cols;

    const shown = state.cursor();
    const cursor = shown !== null && shown.blinking && look.blinkedOut ? null : shown;
    const build = (all: boolean) => {
      this.atlas.reset = false;
      state.forEachRow(all, this.row, (y, row) => {
        if (y >= this.rows) return;
        this.buildRow(y, row, colors.background);
      });
      this.buildCursor(cursor, look.focused, colors.cursor ?? colors.foreground, colors.background);
    };

    // A full atlas clears while rows are built, and the rows built before
    // point at slots now reused: every row is built again, once.
    build(full || this.atlas.reset);
    if (this.atlas.reset) build(true);
    state.clean();

    this.bg.upload();
    this.glyphs.upload();
    this.decorations.upload();

    const placements = state.placements();
    this.images.sync(gl, term, placements);

    gl.viewport(0, 0, this.canvas.width, this.canvas.height);
    const [r, g, b] = rgb(colors.background);
    gl.clearColor(r, g, b, 1);
    gl.clear(gl.COLOR_BUFFER_BIT);

    this.drawImages(placements, "below-background");
    this.drawRects(this.bg, this.rows * cols);
    this.drawImages(placements, "below-text");
    if (cursor !== null && cursor.shape === "block" && look.focused) this.drawRects(this.cursorRect, 1);
    this.drawGlyphs(this.glyphs, this.rows * cols);
    this.drawRects(this.decorations, this.rows * cols * DECORATIONS_PER_CELL);
    this.drawImages(placements, "above-text");

    if (cursor !== null) {
      if (cursor.shape === "block" && look.focused) this.drawGlyphs(this.cursorGlyph, 1);
      else this.drawRects(this.cursorRect, 1);
    }
  }

  /// Whether the GPU context was lost; the screen makes a new renderer.
  lost(): boolean {
    return this.gl.isContextLost();
  }

  dispose() {
    this.images.clear(this.gl);
    this.gl.getExtension("WEBGL_lose_context")?.loseContext();
  }

  private buildRow(y: number, row: RowCells, background: number) {
    const { width: cw, height: ch } = this.cell;
    const cols = this.cols;
    const bg = this.bg.slice(y * cols, cols);
    const glyphs = this.glyphs.slice(y * cols, cols);
    const decorations = this.decorations.slice(y * cols * DECORATIONS_PER_CELL, cols * DECORATIONS_PER_CELL);
    bg.fill(0);
    glyphs.fill(0);
    decorations.fill(0);

    const top = y * ch;
    const line = Math.max(1, Math.round(this.dpr));
    const n = Math.min(cols, row.cols);

    for (let x = 0; x < n; x++) {
      const attrs = row.attrs[x];
      if (attrs & Attr.WIDE_TAIL) continue;

      const span = attrs & Attr.WIDE ? 2 : 1;
      const left = x * cw;
      const cp = row.codepoint[x];
      let cellBg = row.bg[x];
      const selected = (attrs & Attr.SELECTED) !== 0;
      if (selected) cellBg = blend(cellBg, this.theme.selection, this.theme.selectionAlpha);

      if (cellBg !== background) rect(bg, x, left, top, cw * span, ch, cellBg, 1, Kind.SOLID);

      // A kitty placeholder cell shows its image, drawn apart, and no glyph.
      const text = cp === KITTY_PLACEHOLDER ? "" : row.graphemes.get(x) ?? (cp > 32 ? String.fromCodePoint(cp) : "");
      if (text !== "") {
        // An icon may spread into a blank cell after it, as in Ghostty.
        const blankNext = x + 1 < n && row.codepoint[x + 1] <= 32 && !row.graphemes.has(x + 1);
        const room = span === 1 && isIcon(cp) && blankNext ? 2 : span;
        const g = this.atlas.glyph(text, (attrs & Attr.BOLD) !== 0, (attrs & Attr.ITALIC) !== 0, room, this.cell, this.baseline);
        const alpha = attrs & Attr.FAINT ? 0.5 : 1;
        const o = x * GLYPH_FLOATS;
        glyphs.set([left, top, g.width, ch, g.u, g.v, g.uw, g.vh], o);
        const [r, gg, b] = rgb(row.fg[x]);
        glyphs.set([r * alpha, gg * alpha, b * alpha, g.colored ? 1 : 0], o + 8);
      }

      const underline = row.underline(x);
      const d = x * DECORATIONS_PER_CELL;
      if (underline !== "none") {
        const kind = underline === "curly" ? Kind.CURLY : underline === "dotted" ? Kind.DOTTED : underline === "dashed" ? Kind.DASHED : Kind.SOLID;
        const height = underline === "curly" ? line * 3 : underline === "double" ? line * 3 : line;
        const at = top + ch - height - Math.max(0, Math.round(line));
        if (underline === "double") {
          rect(decorations, d, left, at, cw * span, line, row.underlineColor[x], 1, Kind.SOLID);
          rect(decorations, d + 1, left, at + line * 2, cw * span, line, row.underlineColor[x], 1, Kind.SOLID);
        } else {
          rect(decorations, d, left, at, cw * span, height, row.underlineColor[x], 1, kind);
        }
      }
      if (attrs & Attr.STRIKETHROUGH) rect(decorations, d + 2, left, top + Math.round(ch / 2), cw * span, line, row.fg[x], 1, Kind.SOLID);
      if (attrs & Attr.OVERLINE) rect(decorations, d + 3, left, top, cw * span, line, row.fg[x], 1, Kind.SOLID);
    }

    this.bg.dirty(y * cols, cols);
    this.glyphs.dirty(y * cols, cols);
    this.decorations.dirty(y * cols * DECORATIONS_PER_CELL, cols * DECORATIONS_PER_CELL);
  }

  private buildCursor(cursor: Cursor | null, focused: boolean, color: number, background: number) {
    const rects = this.cursorRect.slice(0, 1);
    const glyph = this.cursorGlyph.slice(0, 1);
    rects.fill(0);
    glyph.fill(0);

    if (cursor !== null) {
      const { width: cw, height: ch } = this.cell;
      const left = cursor.x * cw - (cursor.wideTail ? cw : 0);
      const top = cursor.y * ch;
      const line = Math.max(1, Math.round(this.dpr));
      const bar = Math.max(1, Math.round(this.dpr * 1.5));

      if (cursor.shape === "block" && focused) {
        rect(rects, 0, left, top, cw, ch, color, 1, Kind.SOLID);
        // The glyph under it again, in the background colour.
        const cells = this.glyphs.slice(cursor.y * this.cols, this.cols);
        const o = (cursor.x - (cursor.wideTail ? 1 : 0)) * GLYPH_FLOATS;
        glyph.set(cells.subarray(o, o + GLYPH_FLOATS));
        // Text takes the background colour; an emoji keeps its own.
        const [r, g, b] = rgb(background);
        if (glyph[11] === 0) glyph.set([r, g, b], 8);
      } else if (cursor.shape === "block" || cursor.shape === "hollow") {
        rect(rects, 0, left, top, cw, ch, color, 1, Kind.HOLLOW);
      } else if (cursor.shape === "bar") {
        rect(rects, 0, left, top, bar, ch, color, 1, Kind.SOLID);
      } else {
        rect(rects, 0, left, top + ch - line * 2, cw, line * 2, color, 1, Kind.SOLID);
      }
    }

    this.cursorRect.dirty(0, 1);
    this.cursorGlyph.dirty(0, 1);
    this.cursorRect.upload();
    this.cursorGlyph.upload();
  }

  private drawRects(instances: InstanceBuffer, count: number) {
    const gl = this.gl;
    const p = this.rectProgram;
    gl.useProgram(p.program);
    gl.uniform2f(p.uniform("u_view"), this.canvas.width, this.canvas.height);
    this.bindQuad(p);
    instances.bind(p, [["a_rect", 4], ["a_color", 4], ["a_kind", 1]]);
    gl.drawArraysInstanced(gl.TRIANGLE_STRIP, 0, 4, count);
  }

  private drawGlyphs(instances: InstanceBuffer, count: number) {
    const gl = this.gl;
    const p = this.glyphProgram;
    gl.useProgram(p.program);
    gl.uniform2f(p.uniform("u_view"), this.canvas.width, this.canvas.height);
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, this.atlas.texture);
    gl.uniform1i(p.uniform("u_atlas"), 0);
    this.bindQuad(p);
    instances.bind(p, [["a_rect", 4], ["a_uv", 4], ["a_color", 4]]);
    gl.drawArraysInstanced(gl.TRIANGLE_STRIP, 0, 4, count);
  }

  private drawImages(placements: ImagePlacement[], layer: ImagePlacement["layer"]) {
    const gl = this.gl;
    const p = this.glyphProgram;
    const { width: cw, height: ch } = this.cell;

    for (const placement of placements) {
      if (placement.layer !== layer) continue;
      const image = this.images.get(placement.id);
      if (!image) continue;

      const instance = this.image;
      const data = instance.slice(0, 1);
      const s = placement.source;
      data.set([
        placement.col * cw + placement.offsetX,
        placement.row * ch + placement.offsetY,
        placement.width,
        placement.height,
        s.x / image.width,
        s.y / image.height,
        s.width / image.width,
        s.height / image.height,
        0, 0, 0, 1,
      ]);
      instance.dirty(0, 1);
      instance.upload();

      gl.useProgram(p.program);
      gl.uniform2f(p.uniform("u_view"), this.canvas.width, this.canvas.height);
      gl.activeTexture(gl.TEXTURE0);
      gl.bindTexture(gl.TEXTURE_2D, image.texture);
      gl.uniform1i(p.uniform("u_atlas"), 0);
      this.bindQuad(p);
      instance.bind(p, [["a_rect", 4], ["a_uv", 4], ["a_color", 4]]);
      gl.drawArraysInstanced(gl.TRIANGLE_STRIP, 0, 4, 1);
    }

  }

  private bindQuad(p: Program) {
    const gl = this.gl;
    const corner = p.attribute("a_corner");
    gl.bindBuffer(gl.ARRAY_BUFFER, this.quad);
    gl.enableVertexAttribArray(corner);
    gl.vertexAttribPointer(corner, 2, gl.FLOAT, false, 0, 0);
    gl.vertexAttribDivisor(corner, 0);
  }
}

/// A program with its attribute and uniform locations, looked up once.
class Program {
  readonly program: WebGLProgram;
  private readonly gl: WebGL2RenderingContext;
  private readonly attributes = new Map<string, number>();
  private readonly uniforms = new Map<string, WebGLUniformLocation | null>();

  constructor(gl: WebGL2RenderingContext, program: WebGLProgram) {
    this.gl = gl;
    this.program = program;
  }

  attribute(name: string): number {
    let at = this.attributes.get(name);
    if (at === undefined) {
      at = this.gl.getAttribLocation(this.program, name);
      this.attributes.set(name, at);
    }
    return at;
  }

  uniform(name: string): WebGLUniformLocation | null {
    let at = this.uniforms.get(name);
    if (at === undefined) {
      at = this.gl.getUniformLocation(this.program, name);
      this.uniforms.set(name, at);
    }
    return at;
  }
}

/// Instances on the CPU and the GPU; dirty ranges are uploaded alone.
class InstanceBuffer {
  private readonly gl: WebGL2RenderingContext;
  private readonly floats: number;
  private readonly buffer: WebGLBuffer;
  private data = new Float32Array(0);
  private dirtyFrom = Infinity;
  private dirtyTo = 0;
  private sized = false;

  constructor(gl: WebGL2RenderingContext, floats: number) {
    this.gl = gl;
    this.floats = floats;
    this.buffer = must(gl.createBuffer(), "a buffer");
  }

  reserve(instances: number) {
    this.data = new Float32Array(instances * this.floats);
    this.sized = false;
    this.dirtyFrom = 0;
    this.dirtyTo = instances;
  }

  slice(first: number, count: number): Float32Array {
    return this.data.subarray(first * this.floats, (first + count) * this.floats);
  }

  dirty(first: number, count: number) {
    this.dirtyFrom = Math.min(this.dirtyFrom, first);
    this.dirtyTo = Math.max(this.dirtyTo, first + count);
  }

  upload() {
    const gl = this.gl;
    gl.bindBuffer(gl.ARRAY_BUFFER, this.buffer);

    if (!this.sized) {
      gl.bufferData(gl.ARRAY_BUFFER, this.data, gl.DYNAMIC_DRAW);
      this.sized = true;
    } else if (this.dirtyFrom < this.dirtyTo) {
      const from = this.dirtyFrom * this.floats;
      gl.bufferSubData(gl.ARRAY_BUFFER, from * 4, this.data, from, (this.dirtyTo - this.dirtyFrom) * this.floats);
    }

    this.dirtyFrom = Infinity;
    this.dirtyTo = 0;
  }

  /// Binds the instance attributes, `[name, floats]` in their order.
  bind(p: Program, layout: [string, number][]) {
    const gl = this.gl;
    gl.bindBuffer(gl.ARRAY_BUFFER, this.buffer);
    let offset = 0;

    for (const [name, size] of layout) {
      const at = p.attribute(name);
      gl.enableVertexAttribArray(at);
      gl.vertexAttribPointer(at, size, gl.FLOAT, false, this.floats * 4, offset * 4);
      gl.vertexAttribDivisor(at, 1);
      offset += size;
    }
  }
}

interface Glyph {
  u: number;
  v: number;
  uw: number;
  vh: number;
  /// In device pixels: one cell, or two for a wide character.
  width: number;
  colored: boolean;
}

/// Glyphs drawn once into a texture, by text and style.
class GlyphAtlas {
  readonly texture: WebGLTexture;
  /// Set when the atlas was cleared: every row must be drawn again.
  reset = false;
  private readonly gl: WebGL2RenderingContext;
  private readonly scratch: HTMLCanvasElement;
  private readonly ctx: CanvasRenderingContext2D;
  private readonly glyphs = new Map<string, Glyph>();
  private next = 0;
  private slot = { width: 1, height: 1 };
  private fonts = { regular: "", bold: "", italic: "", boldItalic: "" };

  constructor(gl: WebGL2RenderingContext) {
    this.gl = gl;
    this.texture = must(gl.createTexture(), "a texture");
    this.scratch = document.createElement("canvas");
    this.ctx = must(this.scratch.getContext("2d", { willReadFrequently: false }), "a 2D context");

    gl.bindTexture(gl.TEXTURE_2D, this.texture);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, ATLAS_SIZE, ATLAS_SIZE, 0, gl.RGBA, gl.UNSIGNED_BYTE, null);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
  }

  /// Sets the font at `dpr` and gives the cell it makes, in device pixels.
  setFont(font: FontSpec, dpr: number): CellSize & { baseline: number } {
    const px = font.size * dpr;
    this.fonts = {
      regular: `${px}px ${font.family}`,
      bold: `bold ${px}px ${font.family}`,
      italic: `italic ${px}px ${font.family}`,
      boldItalic: `italic bold ${px}px ${font.family}`,
    };

    const ctx = this.ctx;
    ctx.font = this.fonts.regular;
    const m = ctx.measureText("M");
    const width = Math.max(1, Math.round(m.width));
    const height = Math.max(1, Math.round(px * font.lineHeight));
    const ascent = m.fontBoundingBoxAscent;
    const descent = m.fontBoundingBoxDescent;
    const baseline = Math.round((height - (ascent + descent)) / 2 + ascent);

    this.slot = { width: width * 2, height };
    this.scratch.width = this.slot.width;
    this.scratch.height = this.slot.height;
    this.clear();
    return { width, height, baseline };
  }

  glyph(text: string, bold: boolean, italic: boolean, span: number, cell: CellSize, baseline: number): Glyph {
    const key = `${bold ? "b" : ""}${italic ? "i" : ""}${span}${text}`;
    const known = this.glyphs.get(key);
    if (known) return known;

    const perRow = Math.floor(ATLAS_SIZE / this.slot.width);
    const capacity = perRow * Math.floor(ATLAS_SIZE / this.slot.height);
    if (this.next >= capacity) this.clear();

    const index = this.next++;
    const sx = (index % perRow) * this.slot.width;
    const sy = Math.floor(index / perRow) * this.slot.height;
    const width = cell.width * span;
    const colored = draw(this.ctx, text, this.font(bold, italic), width, cell.height, baseline);

    const gl = this.gl;
    gl.bindTexture(gl.TEXTURE_2D, this.texture);
    gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL, true);
    gl.texSubImage2D(gl.TEXTURE_2D, 0, sx, sy, gl.RGBA, gl.UNSIGNED_BYTE, this.scratch);

    const glyph = { u: sx / ATLAS_SIZE, v: sy / ATLAS_SIZE, uw: width / ATLAS_SIZE, vh: cell.height / ATLAS_SIZE, width, colored };
    this.glyphs.set(key, glyph);
    return glyph;
  }

  private font(bold: boolean, italic: boolean): string {
    if (bold && italic) return this.fonts.boldItalic;
    if (bold) return this.fonts.bold;
    if (italic) return this.fonts.italic;
    return this.fonts.regular;
  }

  private clear() {
    this.glyphs.clear();
    this.next = 0;
    this.reset = true;
  }
}

/// Draws `text` into the scratch canvas's top left, `width` by `height`.
/// Box drawing and block elements are drawn as shapes so they meet across
/// cells; the rest with the font. Says whether the glyph keeps its colours.
function draw(ctx: CanvasRenderingContext2D, text: string, font: string, width: number, height: number, baseline: number): boolean {
  ctx.clearRect(0, 0, ctx.canvas.width, ctx.canvas.height);
  ctx.fillStyle = "#fff";
  ctx.strokeStyle = "#fff";

  const cp = text.codePointAt(0) ?? 0;
  if (text.length <= 2 && drawShape(ctx, cp, width, height)) return false;

  ctx.font = font;
  ctx.textBaseline = "alphabetic";
  const metrics = ctx.measureText(text);
  const measured = metrics.width;
  const scale = measured > width ? width / measured : 1;
  ctx.save();

  // An icon too wide for its cells keeps its shape: shrunk both ways and
  // centred on the cell's height. Other glyphs, such as some emoji, are
  // narrowed to fit.
  if (scale < 1 && isIcon(cp)) {
    const middle = (metrics.actualBoundingBoxDescent - metrics.actualBoundingBoxAscent) / 2;
    ctx.translate(0, height / 2 - middle * scale);
    ctx.scale(scale, scale);
  } else {
    ctx.translate(0, baseline);
    ctx.scale(scale, 1);
  }

  ctx.fillText(text, 0, 0);
  ctx.restore();
  return EMOJI.test(text);
}

const EMOJI = /\p{Extended_Pictographic}/u;

/// The quadrants of ▖ to ▟: upper left 1, upper right 2, lower left 4, lower right 8.
const QUADRANTS = [4, 8, 1, 1 | 4 | 8, 1 | 8, 1 | 2 | 4, 1 | 2 | 8, 2, 2 | 4, 2 | 4 | 8];

/// Powerline's triangles, half circles and diagonals, drawn to fill the cell.
function isPowerline(cp: number): boolean {
  return cp >= 0xe0b0 && cp <= 0xe0bf;
}

function drawPowerline(ctx: CanvasRenderingContext2D, cp: number, w: number, h: number) {
  const line = Math.max(1, Math.round(w / 8));
  const shape = (...points: [number, number][]) => {
    ctx.beginPath();
    for (const [x, y] of points) ctx.lineTo(x, y);
  };
  const half = (right: boolean) => {
    ctx.beginPath();
    ctx.ellipse(right ? 0 : w, h / 2, w, h / 2, 0, right ? -Math.PI / 2 : Math.PI / 2, right ? Math.PI / 2 : Math.PI * 1.5);
  };
  ctx.lineWidth = line;

  switch (cp) {
    case 0xe0b0: shape([0, 0], [w, h / 2], [0, h]); ctx.fill(); break;
    case 0xe0b1: shape([0, 0], [w, h / 2], [0, h]); ctx.stroke(); break;
    case 0xe0b2: shape([w, 0], [0, h / 2], [w, h]); ctx.fill(); break;
    case 0xe0b3: shape([w, 0], [0, h / 2], [w, h]); ctx.stroke(); break;
    case 0xe0b4: half(true); ctx.fill(); break;
    case 0xe0b5: half(true); ctx.stroke(); break;
    case 0xe0b6: half(false); ctx.fill(); break;
    case 0xe0b7: half(false); ctx.stroke(); break;
    case 0xe0b8: shape([0, 0], [w, h], [0, h]); ctx.fill(); break;
    case 0xe0ba: shape([w, 0], [w, h], [0, h]); ctx.fill(); break;
    case 0xe0bc: shape([0, 0], [w, 0], [0, h]); ctx.fill(); break;
    case 0xe0be: shape([0, 0], [w, 0], [w, h]); ctx.fill(); break;
    case 0xe0b9: case 0xe0bf: shape([0, 0], [w, h]); ctx.stroke(); break;
    default: shape([w, 0], [0, h]); ctx.stroke();
  }
}

/// Private use characters: Nerd Font icons. Powerline's shapes fill their own cell.
function isIcon(cp: number): boolean {
  return ((cp >= 0xe000 && cp <= 0xf8ff) || cp >= 0xf0000) && !isPowerline(cp);
}

/// Draws block elements and the common box drawing lines; false for other
/// characters.
function drawShape(ctx: CanvasRenderingContext2D, cp: number, w: number, h: number): boolean {
  const fill = (x: number, y: number, fw: number, fh: number) => ctx.fillRect(Math.round(x), Math.round(y), Math.round(fw), Math.round(fh));

  // Block elements.
  switch (cp) {
    case 0x2588: fill(0, 0, w, h); return true;
    case 0x2580: fill(0, 0, w, h / 2); return true;
    case 0x2584: fill(0, h / 2, w, h - Math.round(h / 2)); return true;
    case 0x258c: fill(0, 0, w / 2, h); return true;
    case 0x2590: fill(w / 2, 0, w - Math.round(w / 2), h); return true;
    case 0x2591: case 0x2592: case 0x2593: {
      ctx.globalAlpha = cp === 0x2591 ? 0.25 : cp === 0x2592 ? 0.5 : 0.75;
      fill(0, 0, w, h);
      ctx.globalAlpha = 1;
      return true;
    }
  }
  // Lower eighths ▁ to ▇, and left eighths ▉ to ▏.
  if (cp >= 0x2581 && cp <= 0x2587) { const f = (cp - 0x2580) / 8; fill(0, h - h * f, w, h * f); return true; }
  if (cp >= 0x2589 && cp <= 0x258f) { const f = (0x2590 - cp) / 8; fill(0, 0, w * f, h); return true; }
  // Upper and right eighths ▔ ▕.
  if (cp === 0x2594) { fill(0, 0, w, h / 8); return true; }
  if (cp === 0x2595) { fill(w - w / 8, 0, w / 8, h); return true; }

  // Quadrants ▖ to ▟, split where halves meet in the next cell.
  const quadrants = QUADRANTS[cp - 0x2596];
  if (cp >= 0x2596 && quadrants !== undefined) {
    const mx = Math.round(w / 2);
    const my = Math.round(h / 2);
    if (quadrants & 1) fill(0, 0, mx, my);
    if (quadrants & 2) fill(mx, 0, w - mx, my);
    if (quadrants & 4) fill(0, my, mx, h - my);
    if (quadrants & 8) fill(mx, my, w - mx, h - my);
    return true;
  }

  if (isPowerline(cp)) {
    drawPowerline(ctx, cp, w, h);
    return true;
  }

  const lines = BOX[cp];
  if (lines === undefined) return false;

  // [up, right, down, left], each 0 none, 1 light, 2 heavy.
  const light = Math.max(1, Math.round(w / 8));
  const thick = (weight: number) => (weight === 2 ? light * 2 : light);
  const cx = Math.round(w / 2);
  const cy = Math.round(h / 2);
  const [up, right, down, left] = lines;
  if (up) fill(cx - thick(up) / 2, 0, thick(up), cy + thick(up) / 2);
  if (down) fill(cx - thick(down) / 2, cy - thick(down) / 2, thick(down), h - cy + thick(down) / 2);
  if (left) fill(0, cy - thick(left) / 2, cx + thick(left) / 2, thick(left));
  if (right) fill(cx - thick(right) / 2, cy - thick(right) / 2, w - cx + thick(right) / 2, thick(right));
  return true;
}

/// Box drawing characters by their arms: [up, right, down, left].
const BOX: Record<number, [number, number, number, number]> = {
  0x2500: [0, 1, 0, 1], 0x2501: [0, 2, 0, 2], 0x2502: [1, 0, 1, 0], 0x2503: [2, 0, 2, 0],
  0x250c: [0, 1, 1, 0], 0x250f: [0, 2, 2, 0], 0x2510: [0, 0, 1, 1], 0x2513: [0, 0, 2, 2],
  0x2514: [1, 1, 0, 0], 0x2517: [2, 2, 0, 0], 0x2518: [1, 0, 0, 1], 0x251b: [2, 0, 0, 2],
  0x251c: [1, 1, 1, 0], 0x2523: [2, 2, 2, 0], 0x2524: [1, 0, 1, 1], 0x252b: [2, 0, 2, 2],
  0x252c: [0, 1, 1, 1], 0x2533: [0, 2, 2, 2], 0x2534: [1, 1, 0, 1], 0x253b: [2, 2, 0, 2],
  0x253c: [1, 1, 1, 1], 0x254b: [2, 2, 2, 2],
  0x2574: [0, 0, 0, 1], 0x2575: [1, 0, 0, 0], 0x2576: [0, 1, 0, 0], 0x2577: [0, 0, 1, 0],
  0x2578: [0, 0, 0, 2], 0x2579: [2, 0, 0, 0], 0x257a: [0, 2, 0, 0], 0x257b: [0, 0, 2, 0],
};

/// Image textures by image id, uploaded again when an image's generation
/// changes and dropped when no placement shows it.
class ImageTextures {
  private readonly textures = new Map<number, { texture: WebGLTexture; generation: bigint; width: number; height: number }>();

  sync(gl: WebGL2RenderingContext, term: Terminal, placements: ImagePlacement[]) {
    const shown = new Set<number>();

    for (const p of placements) {
      shown.add(p.id);
      const known = this.textures.get(p.id);
      if (known && known.generation === p.generation) continue;

      const pixels = term.imagePixels(p.id);
      if (!pixels) continue;

      const texture = known?.texture ?? must(gl.createTexture(), "an image texture");
      gl.bindTexture(gl.TEXTURE_2D, texture);
      gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL, true);
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, pixels.width, pixels.height, 0, gl.RGBA, gl.UNSIGNED_BYTE, pixels.rgba);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
      this.textures.set(p.id, { texture, generation: p.generation, width: pixels.width, height: pixels.height });
    }

    for (const [id, known] of this.textures) {
      if (shown.has(id)) continue;
      gl.deleteTexture(known.texture);
      this.textures.delete(id);
    }
  }

  get(id: number) {
    return this.textures.get(id);
  }

  clear(gl: WebGL2RenderingContext) {
    for (const known of this.textures.values()) gl.deleteTexture(known.texture);
    this.textures.clear();
  }
}

function rect(out: Float32Array, index: number, x: number, y: number, w: number, h: number, color: number, alpha: number, kind: number) {
  const [r, g, b] = rgb(color);
  out.set([x, y, w, h, r, g, b, alpha, kind], index * RECT_FLOATS);
}

function rgb(color: number): [number, number, number] {
  return [((color >> 16) & 0xff) / 255, ((color >> 8) & 0xff) / 255, (color & 0xff) / 255];
}

/// `over` at `alpha` over `under`, as 0xRRGGBB.
function blend(under: number, over: number, alpha: number): number {
  const mix = (shift: number) => Math.round(((under >> shift) & 0xff) * (1 - alpha) + ((over >> shift) & 0xff) * alpha);
  return (mix(16) << 16) | (mix(8) << 8) | mix(0);
}

function program(gl: WebGL2RenderingContext, vertex: string, fragment: string): Program {
  const p = must(gl.createProgram(), "a program");
  gl.attachShader(p, shader(gl, gl.VERTEX_SHADER, vertex));
  gl.attachShader(p, shader(gl, gl.FRAGMENT_SHADER, fragment));
  gl.linkProgram(p);
  if (!gl.getProgramParameter(p, gl.LINK_STATUS)) throw new Error(`linking a terminal shader: ${gl.getProgramInfoLog(p)}`);
  return new Program(gl, p);
}

function shader(gl: WebGL2RenderingContext, type: number, source: string): WebGLShader {
  const s = must(gl.createShader(type), "a shader");
  gl.shaderSource(s, source);
  gl.compileShader(s);
  if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error(`compiling a terminal shader: ${gl.getShaderInfoLog(s)}`);
  return s;
}

function buffer(gl: WebGL2RenderingContext, data: Float32Array): WebGLBuffer {
  const b = must(gl.createBuffer(), "a buffer");
  gl.bindBuffer(gl.ARRAY_BUFFER, b);
  gl.bufferData(gl.ARRAY_BUFFER, data, gl.STATIC_DRAW);
  return b;
}

function must<T>(value: T | null, what: string): T {
  if (value === null) throw new Error(`WebGL could not create ${what}`);
  return value;
}
