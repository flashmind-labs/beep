import { composeMessage as compose, fenceSpan, imagePath, lineCount, normalizeNewlines, PASTE_PILL_LINES, pillAt, pillsIn, Pastes, snapCaret, snapSelection } from "./compose-text.ts";
import { invoke } from "../ui/tauri.ts";

// A composer's field as flsh's prompt: a long paste is one pill, removed
// whole and previewed on a click; the part that will go out as code has a
// dim outline; a pasted path to a picture attaches it. The rules are in
// compose-text.ts. Pills and the outline are drawn on a layer behind the
// field, with the same text laid out the same way.

/// The texts behind every field's pills, so a pill copied from one field to
/// another still finds its text.
const pastes = new Pastes();

/// What a composer sends for what is in its field: pills expanded, then fenced.
export function composeMessage(value: string) {
  return compose(value, pastes);
}

export interface ComposeOptions {
  /// Takes a picture whose path was pasted; without it the path stays text.
  images?: (files: File[]) => unknown;
  /// Grows the field with its lines, up to this height in pixels.
  grow?: number;
}

interface Field {
  layer: HTMLDivElement;
  opts: ComposeOptions;
  /// The caret before the last move, to skip a pill in the direction it went.
  caret: number;
  composing: boolean;
  pop: HTMLDivElement | null;
  closePop: (() => void) | null;
  drawing: number;
}

const fields = new WeakMap<HTMLTextAreaElement, Field>();
const VALUE = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!;

/// Makes `field` a composer. Calling it again on the same field does nothing.
export function composeField(field: HTMLTextAreaElement, opts: ComposeOptions = {}) {
  if (fields.has(field)) return;
  const layer = document.createElement("div");
  layer.className = "cx-layer";
  layer.setAttribute("aria-hidden", "true");
  layer.hidden = true;
  field.before(layer);
  field.classList.add("cx-field");
  const parent = field.parentElement;
  if (parent && getComputedStyle(parent).position === "static") parent.style.position = "relative";
  fields.set(field, { layer, opts, caret: field.selectionStart, composing: false, pop: null, closePop: null, drawing: 0 });
  // Text set from code (sending, a queued message put back) draws too.
  Object.defineProperty(field, "value", {
    configurable: true,
    get() { return VALUE.get!.call(this); },
    set(v: string) { VALUE.set!.call(this, v); later(field); },
  });
  field.addEventListener("input", () => { closePop(field); draw(field); });
  field.addEventListener("focus", () => draw(field));
  field.addEventListener("scroll", () => { layer.scrollTop = field.scrollTop; });
  field.addEventListener("paste", e => paste(field, e));
  field.addEventListener("keydown", e => key(field, e));
  field.addEventListener("click", e => click(field, e));
  for (const name of ["selectionchange", "select", "keyup", "mouseup"]) field.addEventListener(name, () => snap(field));
  field.addEventListener("compositionstart", () => { fields.get(field)!.composing = true; });
  field.addEventListener("compositionend", () => { fields.get(field)!.composing = false; });
  draw(field);
}

function later(field: HTMLTextAreaElement) {
  const f = fields.get(field);
  if (!f || f.drawing) return;
  f.drawing = requestAnimationFrame(() => { f.drawing = 0; draw(field); });
}

// ----- Drawing -----

const COPIED = ["fontFamily", "fontSize", "fontWeight", "fontStyle", "fontStretch", "letterSpacing", "wordSpacing", "lineHeight", "textTransform", "textIndent", "tabSize",
  "paddingTop", "paddingRight", "paddingBottom", "paddingLeft"] as const;

function draw(field: HTMLTextAreaElement) {
  const f = fields.get(field);
  if (!f || !field.isConnected) return;
  if (f.opts.grow) {
    // Collapsing the field to measure it makes the log above taller for a
    // moment, which moves a log scrolled to its end.
    const log = document.querySelector("#agbody"), top = log?.scrollTop;
    field.style.height = "auto";
    field.style.height = `${Math.min(field.scrollHeight, f.opts.grow)}px`;
    if (log && log.scrollTop !== top) log.scrollTop = top!;
  }
  const value = field.value, pills = pillsIn(value), fence = fenceSpan(value, pastes), layer = f.layer;
  if (!pills.length && !fence) {
    layer.hidden = true;
    layer.replaceChildren();
    return;
  }
  layer.hidden = false;
  const cs = getComputedStyle(field);
  for (const k of COPIED) layer.style[k] = cs[k];
  layer.style.left = `${field.offsetLeft + field.clientLeft}px`;
  layer.style.top = `${field.offsetTop + field.clientTop}px`;
  layer.style.width = `${field.clientWidth}px`;
  layer.style.height = `${field.clientHeight}px`;
  // Text from `a` to `b`, each pill in it a span.
  const run = (a: number, b: number) => {
    const out: Node[] = [];
    let at = a;
    for (const p of pills) {
      if (p.start < a || p.end > b) continue;
      out.push(document.createTextNode(value.slice(at, p.start)));
      const span = document.createElement("span");
      span.className = "cx-pill";
      span.dataset.id = String(p.id);
      span.textContent = value.slice(p.start, p.end);
      out.push(span);
      at = p.end;
    }
    out.push(document.createTextNode(value.slice(at, b)));
    return out;
  };
  let code: HTMLSpanElement | null = null;
  if (fence) {
    code = document.createElement("span");
    code.append(...run(fence.start, fence.end));
    layer.replaceChildren(...run(0, fence.start), code, ...run(fence.end, value.length));
  } else {
    layer.replaceChildren(...run(0, value.length));
  }
  // A last empty line still takes its height, as in the field.
  layer.append(document.createTextNode(" "));
  layer.scrollTop = field.scrollTop;
  if (code) outline(layer, code);
}

/// A dim box around the lines of `code`, the part sent as a code block.
function outline(layer: HTMLDivElement, code: HTMLSpanElement) {
  const rects = [...code.getClientRects()];
  if (!rects.length) return;
  const top = Math.min(...rects.map(r => r.top)), bottom = Math.max(...rects.map(r => r.bottom));
  const at = layer.getBoundingClientRect();
  const box = document.createElement("div");
  box.className = "cx-code";
  box.style.top = `${top - at.top + layer.scrollTop - 3}px`;
  box.style.height = `${bottom - top + 6}px`;
  layer.append(box);
}

addEventListener("resize", () => document.querySelectorAll<HTMLTextAreaElement>("textarea.cx-field").forEach(later));

// ----- Pills -----

/// Puts `text` where the selection is, as typing would, so undo takes it back.
function insert(field: HTMLTextAreaElement, text: string) {
  field.focus();
  const done = text ? document.execCommand("insertText", false, text) : document.execCommand("delete");
  if (done) return;
  field.setRangeText(text, field.selectionStart, field.selectionEnd, "end");
  field.dispatchEvent(new Event("input", { bubbles: true }));
}

function paste(field: HTMLTextAreaElement, e: ClipboardEvent) {
  const data = e.clipboardData, f = fields.get(field)!;
  // Files go to the attachments, as before.
  if (!data || [...data.items].some(i => i.kind === "file")) return;
  const text = data.getData("text/plain");
  if (!text) return;
  const image = f.opts.images ? imagePath(text) : null;
  if (image) {
    e.preventDefault();
    readImage(image.path).then(file => f.opts.images!([file])).catch(err => {
      // Not a picture that can be read: the path goes in as typed.
      console.warn("pasted picture:", err);
      insert(field, text);
    });
    return;
  }
  const whole = normalizeNewlines(text);
  if (lineCount(whole) <= PASTE_PILL_LINES) return;
  e.preventDefault();
  const before = field.value[field.selectionStart - 1], after = field.value[field.selectionEnd];
  const pad = (c: string | undefined) => (c && !/\s/.test(c) ? " " : "");
  insert(field, `${pad(before)}${pastes.add(whole)}${pad(after)}`);
}

async function readImage(path: string) {
  const got = await invoke<{ name: string; mime: string; data: string }>("read_pasted_image", { path });
  const bin = atob(got.data), bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  return new File([bytes], got.name, { type: got.mime });
}

function key(field: HTMLTextAreaElement, e: KeyboardEvent) {
  const f = fields.get(field)!;
  if (e.key === "Escape" && f.pop) {
    e.preventDefault();
    e.stopPropagation();
    closePop(field);
    return;
  }
  if ((e.key === "ArrowLeft" || e.key === "ArrowRight") && !e.shiftKey && !e.metaKey && !e.altKey && !e.ctrlKey) {
    // Over a pill in one step.
    const at = field.selectionStart, back = e.key === "ArrowLeft";
    const p = at === field.selectionEnd ? pillAt(pillsIn(field.value), at, back) : null;
    if (!p) return;
    e.preventDefault();
    const to = back ? p.start : p.end;
    field.setSelectionRange(to, to);
    f.caret = to;
    return;
  }
  if ((e.key !== "Backspace" && e.key !== "Delete") || e.isComposing) return;
  const at = field.selectionStart;
  if (at !== field.selectionEnd) return;
  const p = pillAt(pillsIn(field.value), at, e.key === "Backspace");
  if (!p) return;
  e.preventDefault();
  field.setSelectionRange(p.start, p.end);
  insert(field, "");
}

// The caret never stops inside a pill, and a selection holds pills whole.
// Browsers fire `selectionchange` at the field or at the document; keys
// and clicks check too.
document.addEventListener("selectionchange", () => {
  if (document.activeElement instanceof HTMLTextAreaElement) snap(document.activeElement);
});

function snap(field: HTMLTextAreaElement) {
  const f = fields.get(field);
  if (!f || f.composing) return;
  const a = field.selectionStart, b = field.selectionEnd, pills = pillsIn(field.value);
  if (a === b) {
    const to = snapCaret(pills, a, f.caret);
    f.caret = to;
    if (to !== a) field.setSelectionRange(to, to);
    return;
  }
  const s = snapSelection(pills, a, b);
  if (s.start !== a || s.end !== b) field.setSelectionRange(s.start, s.end, field.selectionDirection);
}

// ----- Preview -----

/// A click on a pill shows its text, to expand into the field or remove.
function click(field: HTMLTextAreaElement, e: MouseEvent) {
  const f = fields.get(field)!;
  for (const span of f.layer.querySelectorAll<HTMLElement>(".cx-pill")) {
    for (const r of span.getClientRects()) {
      if (e.clientX >= r.left - 2 && e.clientX <= r.right + 2 && e.clientY >= r.top && e.clientY <= r.bottom) return openPop(field, Number(span.dataset.id));
    }
  }
}

const PREVIEW_LINES = 200;

function openPop(field: HTMLTextAreaElement, id: number) {
  const f = fields.get(field)!, text = pastes.get(id);
  closePop(field);
  if (text == null) return;
  const pop = document.createElement("div");
  pop.className = "cx-pop";
  const head = document.createElement("div");
  head.className = "h";
  const name = document.createElement("b");
  name.textContent = "Pasted text";
  const lines = lineCount(text), count = document.createElement("span");
  count.textContent = lines === 1 ? "1 line" : `${lines} lines`;
  const gap = document.createElement("span");
  gap.className = "sp";
  const button = (label: string, act: () => void) => {
    const b = document.createElement("button");
    b.type = "button";
    b.className = "dbtn quiet";
    b.textContent = label;
    b.addEventListener("mousedown", e => e.preventDefault());
    b.addEventListener("click", e => { e.stopPropagation(); act(); });
    return b;
  };
  // The pill's place now: typing may have moved it since the click.
  const select = () => {
    const p = pillsIn(field.value).find(p => p.id === id);
    if (p) field.setSelectionRange(p.start, p.end);
    return !!p;
  };
  head.append(name, count, gap,
    button("Expand", () => { if (select()) insert(field, text); closePop(field); }),
    button("Remove", () => { if (select()) insert(field, ""); closePop(field); }));
  const pre = document.createElement("pre");
  const shown = text.split("\n");
  pre.textContent = shown.length > PREVIEW_LINES ? `${shown.slice(0, PREVIEW_LINES).join("\n")}\n…` : text;
  pop.append(head, pre);
  field.parentElement!.append(pop);
  // Below the field when there is no room above it.
  if (pop.getBoundingClientRect().top < 4) pop.classList.add("down");
  const away = (e: MouseEvent) => { if (!pop.contains(e.target as Node)) closePop(field); };
  addEventListener("mousedown", away, true);
  f.pop = pop;
  f.closePop = () => removeEventListener("mousedown", away, true);
}

function closePop(field: HTMLTextAreaElement) {
  const f = fields.get(field);
  if (!f?.pop) return;
  f.pop.remove();
  f.closePop?.();
  f.pop = null;
  f.closePop = null;
}
