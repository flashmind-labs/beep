import { createDocumentScope } from "../ui/document-root.tsx";
import { ComposeOverlay, ComposePopup } from "./compose-overlay.tsx";
import { composeMessage as compose, expandPills, fenceSpans, imagePath, lineCount, normalizeNewlines, PASTE_PILL_LINES, pillAt, pillsIn, Pastes, snapCaret, snapSelection } from "./compose-text.ts";
import { bytesOf } from "../agentpane/media-state.ts";
import { invoke } from "../ui/tauri.ts";
import { sessionBody } from "../agents/session-frame.tsx";

// A composer's field as flsh's prompt: a long paste is one pill, removed
// whole and previewed on a click; the part that will go out as code has a
// dim outline; a pasted path to a picture attaches it. The rules are in
// compose-text.ts. Pills and the outline are drawn on a layer behind the
// field, with the same text laid out the same way.

/// The texts behind every field's pills, so a pill copied from one field to
/// another still finds its text.
const pastes = new Pastes();
/// The editable draft, including the full text behind paste pills.
export function draftText(value: string) { return expandPills(value, pastes); }

/// What a composer sends for what is in its field: pills expanded, then fenced.
export function composeMessage(value: string) {
  return compose(value, pastes);
}

interface ComposeOptions {
  /// A stable key for a prompt kept locally between visits and app launches.
  draft?: string;
  /// Takes a picture whose path was pasted; without it the path stays text.
  images?: (files: File[]) => unknown;
  /// Grows the field with its lines, up to this height in pixels.
  grow?: number;
  /// Takes the last attachment off the message: a Backspace at the start of the field.
  removeLast?: () => void;
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
  slot: string;
  shown: boolean;
  active: boolean;
}

const fields = new WeakMap<HTMLTextAreaElement, Field>();
const overlay = createDocumentScope<string>();
let nextSlot = 0;
// Cached fields may leave the document and return, and hidden ones show nothing.
// Release portal ownership while a field is out of view and draw it again when
// it returns. Fields are held weakly, so discarded ones are not retained.
function seen(entries: IntersectionObserverEntry[]) {
  for (const { target, isIntersecting } of entries) {
    if (!(target instanceof HTMLTextAreaElement)) continue;
    const state = fields.get(target);
    if (!state || state.shown === isIntersecting) continue;

    state.shown = isIntersecting;
    if (state.shown) { onScreen.add(target); draw(target); }
    else {
      onScreen.delete(target);
      closePop(target);
      if (state.active) { overlay.remove([state.slot]); state.active = false; }
    }
  }
}
/// Made with the first composer, so the module loads without a DOM. A DOM
/// without layout has none, and its overlays stay as drawn.
let views: IntersectionObserver | null = null;
const VALUE = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!;

const draftKeyOf = (draft: string) => `beep.promptDraft.v1.${draft}`;

/// Puts `text` in front of the draft kept for a field not on screen, as `ComposeOptions.draft`
/// names it, so it shows when that field is drawn.
export function prependDraft(draft: string, text: string) {
  try {
    const kept = localStorage.getItem(draftKeyOf(draft));
    localStorage.setItem(draftKeyOf(draft), kept ? `${text}\n${kept}` : text);
  } catch { /* Storage can be unavailable. */ }
}

/// Makes `field` a composer. Calling it again on the same field does nothing.
export function composeField(field: HTMLTextAreaElement, opts: ComposeOptions = {}) {
  if (fields.has(field)) return;
  const draftKey = opts.draft === undefined ? null : draftKeyOf(opts.draft);

  if (draftKey) {
    try { field.value = localStorage.getItem(draftKey) ?? field.value; } catch { /* Storage can be unavailable. */ }
  }

  const saveDraft = () => {
    if (!draftKey) return;

    try {
      // Pill IDs only live in this window. Keep their text without changing
      // whitespace or adding the code fences that sending would add.
      const text = expandPills(field.value, pastes);
      if (text) localStorage.setItem(draftKey, text);
      else localStorage.removeItem(draftKey);
    } catch { /* Keep the field usable when storage is full or unavailable. */ }
  };
  const layer = document.createElement("div");
  layer.className = "cx-layer";
  layer.setAttribute("aria-hidden", "true");
  layer.hidden = true;
  field.before(layer);
  field.classList.add("cx-field");
  const parent = field.parentElement;
  if (parent && getComputedStyle(parent).position === "static") parent.style.position = "relative";
  fields.set(field, { layer, opts, caret: field.selectionStart, composing: false, pop: null, closePop: null, drawing: 0, slot: String(++nextSlot), shown: true, active: false });
  if (typeof IntersectionObserver === "function") views ??= new IntersectionObserver(seen);
  views?.observe(field);
  onScreen.add(field);
  // Text set from code (sending, a queued message put back) draws too.
  Object.defineProperty(field, "value", {
    configurable: true,
    get() { return VALUE.get!.call(this); },
    set(v: string) { VALUE.set!.call(this, v); saveDraft(); later(field); },
  });
  field.addEventListener("input", () => { saveDraft(); closePop(field); draw(field); });
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
    const log = sessionBody(), top = log?.scrollTop;
    field.style.height = "auto";
    field.style.height = `${Math.min(field.scrollHeight, f.opts.grow)}px`;
    if (log && log.scrollTop !== top) log.scrollTop = top!;
  }

  const value = field.value, pills = pillsIn(value), fences = fenceSpans(value, pastes), layer = f.layer;

  if (!pills.length && !fences.length) {
    layer.hidden = true;
    if (f.active) { overlay.remove([f.slot]); f.active = false; }
    return;
  }

  layer.hidden = false;
  const cs = getComputedStyle(field);
  for (const k of COPIED) layer.style[k] = cs[k];
  layer.style.left = `${field.offsetLeft + field.clientLeft}px`;
  layer.style.top = `${field.offsetTop + field.clientTop}px`;
  layer.style.width = `${field.clientWidth}px`;
  layer.style.height = `${field.clientHeight}px`;
  overlay.set([{ slot: f.slot, host: layer, content: <ComposeOverlay value={value} pills={pills} fences={fences} /> }]);
  f.active = true;
  layer.scrollTop = field.scrollTop;
}

/// The composers on screen, which a resize lays out again.
const onScreen = new Set<HTMLTextAreaElement>();
addEventListener("resize", () => {
  for (const field of onScreen) {
    // A field that left the page is added again when it shows.
    if (field.isConnected) later(field);
    else onScreen.delete(field);
  }
});

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
  return new File([bytesOf(got.data)], got.name, { type: got.mime });
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

  if (at === 0 && e.key === "Backspace") {
    f.opts.removeLast?.();
    return;
  }

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
  // The pill's place now: typing may have moved it since the click.
  const select = () => {
    const p = pillsIn(field.value).find(p => p.id === id);
    if (p) field.setSelectionRange(p.start, p.end);
    return !!p;
  };
  const shown = text.split("\n");
  const preview = shown.length > PREVIEW_LINES ? `${shown.slice(0, PREVIEW_LINES).join("\n")}\n…` : text;
  const parent = field.parentElement;
  if (!parent) return;
  overlay.set([{ slot: `popup:${f.slot}`, host: parent, content: <ComposePopup
    text={preview} lines={lineCount(text)} node={element => { f.pop = element; }}
    expand={() => { if (select()) insert(field, text); closePop(field); }}
    remove={() => { if (select()) insert(field, ""); closePop(field); }} /> }]);
  const pop = f.pop;
  if (!pop) return;
  const away = (e: MouseEvent) => { if (!pop.contains(e.target as Node)) closePop(field); };
  addEventListener("mousedown", away, true);
  f.closePop = () => removeEventListener("mousedown", away, true);
}

function closePop(field: HTMLTextAreaElement) {
  const f = fields.get(field);
  if (!f?.pop) return;
  overlay.remove([`popup:${f.slot}`]);
  f.closePop?.();
  f.pop = null;
  f.closePop = null;
}
