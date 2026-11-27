import { useCallback, useEffect, useLayoutEffect, useRef, useState, type CSSProperties, type Ref, type TextareaHTMLAttributes } from "react";
import { NO_AUTO } from "../../ui/kit.tsx";
import { onPhone } from "../../ui/platform.ts";
import { ComposeOverlay, ComposePopup } from "./compose-overlay.tsx";
import { composeMessage as compose, expandPills, fenceSpans, imagePath, lineCount, normalizeNewlines, PASTE_PILL_LINES, pillAt, pillsIn, Pastes, quoteAppend, snapCaret, snapSelection } from "../../stores/chat/compose-text.ts";
import { bytesOf } from "../../stores/agentpane/media-state.ts";
import { invoke } from "../../ui/tauri.ts";
import { sessionBody } from "../../stores/agents/session-frame-state.ts";

/// Phones keep the keyboard's autocorrect and capitals for prose prompts.
const PHONE_TYPING = { autoComplete: "off", autoCorrect: "on", autoCapitalize: "sentences", spellCheck: true } as const;

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
  /// The field is keyed by draft so each prompt gets its own native editor.
  draft?: string;
  /// Takes a picture whose path was pasted; without it the path stays text.
  images?: (files: File[]) => unknown;
  /// Grows the field with its lines, up to this height in pixels.
  grow?: number;
  /// Takes the last attachment off the message: a Backspace at the start of the field.
  removeLast?: () => void;
}

type ComposeFieldProps = ComposeOptions & Omit<TextareaHTMLAttributes<HTMLTextAreaElement>, "value" | "defaultValue"> & { ref?: Ref<HTMLTextAreaElement> };

const VALUE = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value");
const draftKeyOf = (draft: string) => `beep.promptDraft.v1.${draft}`;

function keptDraft(key: string | null) {
  try { return key ? localStorage.getItem(key) ?? "" : ""; } catch { return ""; }
}

/// Puts `text` in front of the draft kept for a field not on screen, as `ComposeOptions.draft`
/// names it, so it shows when that field is drawn.
export function prependDraft(draft: string, text: string) {
  try {
    const kept = localStorage.getItem(draftKeyOf(draft));
    localStorage.setItem(draftKeyOf(draft), kept ? `${text}\n${kept}` : text);
  } catch { /* Storage can be unavailable. */ }
}

/// Starts the field on screen with `text`, unless it starts with it already, and leaves the
/// caret at the end to go on typing.
export function startField(field: HTMLTextAreaElement, text: string) {
  field.focus();

  if (!field.value.startsWith(text)) {
    field.setSelectionRange(0, 0);
    insert(field, text);
  }

  field.setSelectionRange(field.value.length, field.value.length);
}

/// Appends `selected` to `field` as a Markdown blockquote (`> ...`) and
/// focuses `field` on the blank line below it, keeping undo intact.
export function appendQuote(field: HTMLTextAreaElement, selected: string): boolean {
  const edit = quoteAppend(field.value, selected);
  if (!edit) return false;

  field.setSelectionRange(edit.from, field.value.length);
  insert(field, edit.text);
  field.setSelectionRange(field.value.length, field.value.length);
  return true;
}

/// A composer's field: a textarea with the layer behind it that draws its
/// pills and code outlines, and the preview of a clicked pill.
export function ComposeField({ draft, images, grow, removeLast, ref, className, onInput, onFocus, onScroll, onPaste, onKeyDown, onClick, onSelect, onKeyUp, onMouseUp, onCompositionStart, onCompositionEnd, ...rest }: ComposeFieldProps) {
  const field = useRef<HTMLTextAreaElement | null>(null), layer = useRef<HTMLDivElement>(null), pillNodes = useRef(new Map<number, HTMLSpanElement>());
  const draftKey = draft === undefined ? null : draftKeyOf(draft);
  const [initial] = useState(() => keptDraft(draftKey));
  const [value, setValue] = useState(initial);
  const [box, setBox] = useState<CSSProperties | null>(null);
  const [size, setSize] = useState(0);
  const [pop, setPop] = useState<number | null>(null);
  /// The caret before the last move, to skip a pill in the direction it went.
  const caret = useRef(0), composing = useRef(false);
  const pills = pillsIn(value), fences = fenceSpans(value, pastes), shown = pills.length > 0 || fences.length > 0;

  const saveDraft = (text: string) => {
    if (!draftKey) return;

    try {
      // Pill IDs only live in this window. Keep their text without changing
      // whitespace or adding the code fences that sending would add.
      const kept = expandPills(text, pastes);
      if (kept) localStorage.setItem(draftKey, kept);
      else localStorage.removeItem(draftKey);
    } catch { /* Keep the field usable when storage is full or unavailable. */ }
  };
  const changed = (text: string) => { saveDraft(text); setValue(text); };

  // Text set from code (sending, a queued message put back, the mention menu)
  // draws and saves too. Callers set `value` directly; until they all go
  // through props, the field reports those writes here.
  const attach = useCallback((node: HTMLTextAreaElement | null) => {
    field.current = node;
    if (typeof ref === "function") ref(node); else if (ref) ref.current = node;
    if (!node || !VALUE?.get || !VALUE.set) return;

    const { get, set } = VALUE;
    Object.defineProperty(node, "value", { configurable: true, get() { return get.call(this); }, set(text: string) { set.call(this, text); changed(text); } });
  }, [ref, draftKey]);

  const closePop = useCallback(() => setPop(null), []);

  useLayoutEffect(() => {
    const node = field.current;
    if (!node || !grow) return;

    // Collapsing the field to measure it makes the log above taller for a
    // moment, which moves a log scrolled to its end.
    const log = sessionBody(), top = log?.scrollTop ?? 0;
    node.style.height = "auto";
    node.style.minHeight = "0px";
    const height = Math.min(node.scrollHeight, grow);
    node.style.minHeight = "";
    node.style.height = `${height}px`;
    if (log && log.scrollTop !== top) log.scrollTop = top;
  }, [value, grow, size]);

  // The layer takes the field's place and type, so the same text lays out the same way.
  useLayoutEffect(() => {
    const node = field.current;
    if (!node || !shown) return;

    const cs = getComputedStyle(node);
    const next: CSSProperties = { left: node.offsetLeft + node.clientLeft, top: node.offsetTop + node.clientTop, width: node.clientWidth, height: node.clientHeight };
    for (const k of COPIED) next[k] = cs[k];
    setBox(old => old && COPIED_BOX.every(k => old[k] === next[k]) ? old : next);
    if (layer.current) layer.current.scrollTop = node.scrollTop;
  });

  // A resize, of the window or of the field moved to another host, lays it out again.
  useEffect(() => {
    const node = field.current;
    if (!node || typeof ResizeObserver !== "function") return;

    const seen = new ResizeObserver(() => setSize(n => n + 1));
    seen.observe(node);
    return () => seen.disconnect();
  }, []);

  const snapNow = () => {
    const node = field.current;
    if (node && !composing.current) caret.current = snap(node, caret.current);
  };

  return <>
    <div ref={layer} className="cx-layer" aria-hidden="true" hidden={!shown} style={box ?? undefined}>{shown && <ComposeOverlay value={value} pills={pills} fences={fences} pillNode={(id, node) => { if (node) pillNodes.current.set(id, node); else pillNodes.current.delete(id); }} />}</div>
    <textarea ref={attach} defaultValue={initial} className={className ? `${className} cx-field` : "cx-field"} {...(onPhone() ? PHONE_TYPING : NO_AUTO)} {...rest}
      onInput={event => { changed(event.currentTarget.value); setPop(null); onInput?.(event); }}
      onFocus={event => { setSize(n => n + 1); onFocus?.(event); }}
      onScroll={event => { if (layer.current) layer.current.scrollTop = event.currentTarget.scrollTop; onScroll?.(event); }}
      onPaste={event => { paste(event.currentTarget, event.nativeEvent, images); onPaste?.(event); }}
      onKeyDown={event => {
        const node = event.currentTarget;
        if (event.key === "Escape" && pop !== null) { event.preventDefault(); event.stopPropagation(); setPop(null); return; }
        caret.current = key(node, event.nativeEvent, caret.current, removeLast);
        onKeyDown?.(event);
      }}
      onClick={event => {
        // The pills lie under the field; a click on one shows its text.
        const at = [...pillNodes.current].find(([, span]) => [...span.getClientRects()].some(r => event.clientX >= r.left - 2 && event.clientX <= r.right + 2 && event.clientY >= r.top && event.clientY <= r.bottom));
        if (at) setPop(at[0]);
        onClick?.(event);
      }}
      onSelect={event => { snapNow(); onSelect?.(event); }}
      onKeyUp={event => { snapNow(); onKeyUp?.(event); }}
      onMouseUp={event => { snapNow(); onMouseUp?.(event); }}
      onCompositionStart={event => { composing.current = true; onCompositionStart?.(event); }}
      onCompositionEnd={event => { composing.current = false; onCompositionEnd?.(event); }} />
    {pop !== null && field.current && <PillPreview field={field.current} id={pop} close={closePop} />}
  </>;
}

const COPIED = ["fontFamily", "fontSize", "fontWeight", "fontStyle", "fontStretch", "letterSpacing", "wordSpacing", "lineHeight", "textTransform", "textIndent", "tabSize",
  "paddingTop", "paddingRight", "paddingBottom", "paddingLeft"] as const;
const COPIED_BOX = [...COPIED, "left", "top", "width", "height"] as const;

// ----- Pills -----

/// Puts `text` where the selection is, as typing would, so undo takes it back.
function insert(field: HTMLTextAreaElement, text: string) {
  field.focus();
  const done = text ? document.execCommand("insertText", false, text) : document.execCommand("delete");
  if (done) return;
  field.setRangeText(text, field.selectionStart, field.selectionEnd, "end");
  field.dispatchEvent(new Event("input", { bubbles: true }));
}

function paste(field: HTMLTextAreaElement, e: ClipboardEvent, images: ComposeOptions["images"]) {
  const data = e.clipboardData;
  // Files go to the attachments, as before.
  if (!data || data.files.length || [...data.items].some(i => i.kind === "file")) return;
  const text = data.getData("text/plain");
  if (!text) return;
  const image = images ? imagePath(text) : null;

  if (images && image) {
    e.preventDefault();
    readImage(image.path).then(file => images([file])).catch(err => {
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

/// Steps over a pill, or removes it whole. Returns where the caret is.
function key(field: HTMLTextAreaElement, e: KeyboardEvent, caret: number, removeLast: ComposeOptions["removeLast"]) {
  if ((e.key === "ArrowLeft" || e.key === "ArrowRight") && !e.shiftKey && !e.metaKey && !e.altKey && !e.ctrlKey) {
    const at = field.selectionStart, back = e.key === "ArrowLeft";
    const p = at === field.selectionEnd ? pillAt(pillsIn(field.value), at, back) : null;
    if (!p) return caret;
    e.preventDefault();
    const to = back ? p.start : p.end;
    field.setSelectionRange(to, to);
    return to;
  }

  if ((e.key !== "Backspace" && e.key !== "Delete") || e.isComposing) return caret;
  const at = field.selectionStart;
  if (at !== field.selectionEnd) return caret;

  if (at === 0 && e.key === "Backspace") {
    removeLast?.();
    return caret;
  }

  const p = pillAt(pillsIn(field.value), at, e.key === "Backspace");
  if (!p) return caret;
  e.preventDefault();
  field.setSelectionRange(p.start, p.end);
  insert(field, "");
  return caret;
}

/// The caret never stops inside a pill, and a selection holds pills whole.
/// Returns where the caret is.
function snap(field: HTMLTextAreaElement, caret: number) {
  const a = field.selectionStart, b = field.selectionEnd, pills = pillsIn(field.value);

  if (a === b) {
    const to = snapCaret(pills, a, caret);
    if (to !== a) field.setSelectionRange(to, to);
    return to;
  }

  const s = snapSelection(pills, a, b);
  if (s.start !== a || s.end !== b) field.setSelectionRange(s.start, s.end, field.selectionDirection);
  return caret;
}

// ----- Preview -----

const PREVIEW_LINES = 200;

/// A pill's text, to expand into the field or remove. A press outside closes it.
function PillPreview({ field, id, close }: { field: HTMLTextAreaElement; id: number; close(): void }) {
  const pop = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    const away = (e: MouseEvent) => { if (!(e.target instanceof Node && pop.current?.contains(e.target))) close(); };
    addEventListener("mousedown", away, true);
    return () => removeEventListener("mousedown", away, true);
  }, [close]);

  const text = pastes.get(id);
  if (text == null) return null;

  // The pill's place now: typing may have moved it since the click.
  const select = () => {
    const p = pillsIn(field.value).find(p => p.id === id);
    if (p) field.setSelectionRange(p.start, p.end);
    return !!p;
  };
  const shown = text.split("\n");
  const preview = shown.length > PREVIEW_LINES ? `${shown.slice(0, PREVIEW_LINES).join("\n")}\n…` : text;

  return <ComposePopup text={preview} lines={lineCount(text)} node={element => { pop.current = element; }}
    expand={() => { if (select()) insert(field, text); close(); }}
    remove={() => { if (select()) insert(field, ""); close(); }} />;
}
