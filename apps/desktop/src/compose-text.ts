// What a composer does to its text, as flsh does in its prompt: a long
// paste stands in the field as one pill that goes out as the full text,
// text between two triple line breaks goes out in a code fence, and a
// pasted path to a picture attaches the picture. No DOM here.

/// A paste of more lines than this becomes a pill.
export const PASTE_PILL_LINES = 10;
/// How many pasted texts are kept for pills still in some field.
const KEPT_PASTES = 50;

/// `text` with every line break as `\n`.
export function normalizeNewlines(text: string) {
  return text.replace(/\r\n?/g, "\n");
}

/// How many lines `text` has, counted as Rust's `str::lines` does: a final
/// line break does not start another line.
export function lineCount(text: string) {
  if (!text) return 0;
  const n = text.split("\n").length;
  return text.endsWith("\n") ? n - 1 : n;
}

/// Fences the text between the first two triple line breaks: "prompt", three
/// line breaks, the content, three more, then the rest.
export function fenceAfterBlankLine(text: string) {
  const trimmed = text.trim();
  if (trimmed.startsWith("/") || trimmed.startsWith("!") || text.includes("```") || text.includes("~~~")) return trimmed;
  const at = text.indexOf("\n\n\n");
  if (at < 0) return trimmed;
  const prompt = text.slice(0, at).trim(), rest = text.slice(at + 3);
  const end = rest.indexOf("\n\n\n");
  const content = (end < 0 ? rest : rest.slice(0, end)).trimEnd();
  if (!content) return trimmed;
  let fenced = prompt ? `${prompt}\n\n\n\`\`\`\n${content}\n\`\`\`` : `\`\`\`\n${content}\n\`\`\``;
  const suffix = end < 0 ? "" : rest.slice(end + 3).trimEnd();
  if (suffix) fenced += `\n\n\n${suffix}`;
  return fenced;
}

// ----- Pasted text pills -----

// A pill is its label in the field's text: "Pasted text · 42 lines" between
// word joiners, with its id written in zero-width characters, so the field
// shows only the label and a copy of it still finds its text.
const MARK = "⁠", ZERO = "​", ONE = "‌";
const PILL = /⁠Pasted text · (\d+) lines?⁠([​‌]+)⁠/g;

/// A pill in a field's text, `start` to `end`.
export interface Pill {
  id: number;
  lines: number;
  start: number;
  end: number;
}

/// The text a pill stands for, by id.
export class Pastes {
  private texts = new Map<number, string>();
  private next = 1;

  /// Keeps `text` and returns the pill's label for it.
  add(text: string) {
    const id = this.next++;
    this.texts.set(id, text);
    for (const old of this.texts.keys()) {
      if (this.texts.size <= KEPT_PASTES) break;
      this.texts.delete(old);
    }
    return pillLabel(id, lineCount(text));
  }

  get(id: number) {
    return this.texts.get(id);
  }
}

/// The label a pill shows in the field.
export function pillLabel(id: number, lines: number) {
  return `${MARK}Pasted text · ${lines === 1 ? "1 line" : `${lines} lines`}${MARK}${id.toString(2).replace(/0/g, ZERO).replace(/1/g, ONE)}${MARK}`;
}

/// The pills in `value`, in order.
export function pillsIn(value: string): Pill[] {
  return [...value.matchAll(PILL)].map(m => ({
    id: parseInt(m[2].replace(/​/g, "0").replace(/‌/g, "1"), 2),
    lines: Number(m[1]),
    start: m.index,
    end: m.index + m[0].length,
  }));
}

/// `value` with each pill as the text it stands for. A pill whose text is
/// gone keeps its visible label.
export function expandPills(value: string, pastes: Pick<Pastes, "get">) {
  let out = "", at = 0;
  for (const p of pillsIn(value)) {
    out += value.slice(at, p.start);
    out += pastes.get(p.id) ?? `[Pasted text · ${p.lines === 1 ? "1 line" : `${p.lines} lines`}]`;
    at = p.end;
  }
  return out + value.slice(at);
}

/// What a composer sends for `value`: pills expanded, then fenced.
export function composeMessage(value: string, pastes: Pick<Pastes, "get">) {
  return fenceAfterBlankLine(expandPills(value, pastes));
}

/// Where the code fence goes in `value`, from the start of its content to
/// its trimmed end, or null when sending adds none.
export function fenceSpan(value: string, pastes: Pick<Pastes, "get">) {
  const expanded = expandPills(value, pastes);
  if (fenceAfterBlankLine(expanded) === expanded.trim()) return null;
  const at = value.indexOf("\n\n\n");
  if (at < 0) return null;
  const start = at + 3, next = value.indexOf("\n\n\n", start);
  let end = next < 0 ? value.length : next;
  while (end > start && /\s/.test(value[end - 1])) end--;
  return end > start ? { start, end } : null;
}

/// A caret at `pos`, moved out of any pill it fell into: past it when it
/// came from before the pill, else in front of it.
export function snapCaret(pills: readonly Pill[], pos: number, prev: number) {
  const p = pills.find(p => pos > p.start && pos < p.end);
  if (!p) return pos;
  if (prev <= p.start) return p.end;
  if (prev >= p.end) return p.start;
  return pos - p.start < p.end - pos ? p.start : p.end;
}

/// A selection widened to hold every pill it touches whole.
export function snapSelection(pills: readonly Pill[], start: number, end: number) {
  for (const p of pills) {
    if (start > p.start && start < p.end) start = p.start;
    if (end > p.start && end < p.end) end = p.end;
  }
  return { start, end };
}

/// The pill a Backspace (`back`) or Delete at caret `pos` removes whole.
export function pillAt(pills: readonly Pill[], pos: number, back: boolean) {
  return pills.find(p => (back ? p.end : p.start) === pos) ?? null;
}

// ----- Pasted picture paths -----

const IMAGE_TYPES: Record<string, string> = { png: "image/png", jpg: "image/jpeg", jpeg: "image/jpeg", gif: "image/gif", webp: "image/webp" };

/// The picture a paste names when it is just a path to one: quoted, as a
/// `file://` URL or with backslash-escaped spaces, as terminals and Finder
/// give them. Only full paths (`/` or `~/`) count.
export function imagePath(text: string): { path: string; mime: string } | null {
  let t = text.trim();
  if (!t || t.includes("\n")) return null;
  const quoted = /^'(.*)'$/.exec(t) ?? /^"(.*)"$/.exec(t);
  if (quoted) t = quoted[1];
  if (t.startsWith("file://")) {
    t = t.slice("file://".length);
    try { t = decodeURIComponent(t); } catch { return null; }
  }
  t = t.replace(/\\ /g, " ");
  if (!t.startsWith("/") && !t.startsWith("~/")) return null;
  const ext = /\.([A-Za-z0-9]+)$/.exec(t)?.[1].toLowerCase();
  const mime = ext ? IMAGE_TYPES[ext] : undefined;
  return mime ? { path: t, mime } : null;
}
