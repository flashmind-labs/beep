// An agent's report, read from its reply's HTML: the reply that ends a
// finished turn, written to Beep's `report` skill (crates/node/skills/report).
// It opens with its outcome (`Done`, `Partly done`, `Blocked`), then the
// headline, then `##` sections the view draws as parts.
import { Element, Text, htmlToDOM, type DOMNode } from "html-react-parser";

// By type, not class: the parser's CommonJS and ES builds each load domhandler's.
const isTag = (node: { type: string } | undefined): node is Element => node?.type === "tag" || node?.type === "script" || node?.type === "style";
const isText = (node: { type: string } | undefined): node is Text => node?.type === "text";

export type Outcome = "done" | "partly" | "blocked";

/// What a check under Checked found, by the first word of its result.
export type Check = "pass" | "fail" | "unchecked";

export type SectionKind = "needs" | "changed" | "checked" | "notdone" | "other";

export interface ReportSection {
  title: string;
  kind: SectionKind;
  /// Its top-level list items.
  count: number;
  nodes: DOMNode[];
}

/// A report in the skill's shape. Its nodes are the reply's own, or copies
/// of the parts of the first paragraph.
export interface Shaped {
  outcome: Outcome;
  headline: DOMNode[];
  intro: DOMNode[];
  sections: ReportSection[];
}

/// A finished turn's last reply this long counts as a report without the shape.
export const REPORT_MIN_WORDS = 80;

/// Words read a minute, for a report's reading time.
const READING_WPM = 230;

export const OUTCOME_LABEL: Record<Outcome, string> = { done: "Done", partly: "Partly done", blocked: "Blocked" };

/// The reply's length and reading time: "80 words, about 1 min".
export function readingTime(text: string) {
  const words = text.split(/\s+/).filter(Boolean).length;
  return { words, minutes: Math.max(1, Math.round(words / READING_WPM)) };
}

/// A finished turn's last reply as a report: in the skill's shape, or long
/// enough to read as one without it.
export type Report = { kind: "shaped"; words: number; minutes: number; shaped: Shaped } | { kind: "long"; words: number; minutes: number; nodes: DOMNode[] };

/// The reply with sanitized `html` and source `text` as a report, or null
/// when it is neither in the shape nor long.
export function reportOf(html: string, text: string): Report | null {
  const { words, minutes } = readingTime(text);
  const nodes = htmlToDOM(html);
  const shaped = readReport(nodes);
  if (shaped) return { kind: "shaped", words, minutes, shaped };
  return words >= REPORT_MIN_WORDS ? { kind: "long", words, minutes, nodes } : null;
}

/// `nodes` read as a report, or null when they don't open with an outcome.
export function readReport(nodes: DOMNode[]): Shaped | null {
  const start = nodes.findIndex(node => !(isText(node) && !node.data.trim()));
  const first = nodes[start];
  if (!(isTag(first)) || first.name !== "p") return null;

  const masked = maskedText(first.children as DOMNode[]);
  const opened = outcomeOf(masked);
  if (!opened) return null;

  // The headline is the first sentence: a period and a space, outside code.
  const stop = masked.indexOf(". ", opened.from);
  const end = stop < 0 ? masked.length : stop + 1;
  const kids = first.children as DOMNode[];
  const headline = slice(kids, opened.from, end);
  const rest = slice(kids, end, masked.length);
  const restText = rest.map(textOf).join("").trim();
  const intro: DOMNode[] = restText ? [new Element("p", { ...first.attribs }, trimStart(rest))] : [];
  const sections: ReportSection[] = [];

  for (const node of nodes.slice(start + 1)) {
    if (isTag(node) && node.name === "h2") {
      const title = textOf(node).trim();
      sections.push({ title, kind: kindOf(title), count: 0, nodes: [] });
    } else if (sections.length) {
      sections[sections.length - 1].nodes.push(node);
    } else {
      intro.push(node);
    }
  }

  for (const section of sections) {
    section.count = section.nodes.reduce((n, node) => n + (isList(node) ? items(node).length : 0), 0);
    if (section.kind === "checked") section.nodes = section.nodes.map(markChecks);
  }

  return { outcome: opened.outcome, headline: capitalized(trimStart(headline)), intro, sections };
}

/// A check's result, from its text: "**`cargo test`:** passes (214 tests)".
export function checkOf(text: string): Check | null {
  const lower = text.trim().toLowerCase();
  if (/^(not checked|not tested|not run|unchecked|untested)\b/.test(lower)) return "unchecked";

  const colon = lower.indexOf(":");
  const result = (colon < 0 ? lower : lower.slice(colon + 1)).trimStart();
  const word = /^[a-z0-9]*/.exec(result)?.[0] ?? "";
  if (PASS.has(word)) return "pass";
  if (FAIL.has(word)) return "fail";
  return null;
}

const PASS = new Set(["pass", "passes", "passed", "passing", "ok", "works", "worked", "succeeds", "succeeded", "green", "clean"]);
const FAIL = new Set(["fail", "fails", "failed", "failing", "broken", "breaks", "errors", "red"]);

/// The outcome a report's first paragraph opens with, and where its
/// result starts. A few words may sit before the colon ("Done in the
/// lock:"), but not after a bare "Found", which starts a sentence.
export function outcomeOf(masked: string): { outcome: Outcome; from: number } | null {
  const head = /^\s*(partly done|partially done|done|found it|found|blocked)/i.exec(masked);
  if (!head) return null;

  const word = head[1].toLowerCase();
  const outcome: Outcome = word.startsWith("part") ? "partly" : word === "blocked" ? "blocked" : "done";
  const after = masked.slice(head[0].length);
  const marked = /^(:| —| -)\s*/.exec(after);
  if (marked) return { outcome, from: head[0].length + marked[0].length };

  const qualified = /^([ ,][^:.\n\0]*):\s*/.exec(after);
  const words = qualified?.[1].split(/\s+/).filter(Boolean).length ?? 0;
  if (!qualified || word === "found" || words < 1 || words > 5) return null;
  return { outcome, from: head[0].length + qualified[0].length };
}

function kindOf(title: string): SectionKind {
  switch (title.toLowerCase()) {
    case "needs you": return "needs";
    case "changed": return "changed";
    case "checked": return "checked";
    case "not done": return "notdone";
    default: return "other";
  }
}

const isList = (node: DOMNode): node is Element => isTag(node) && (node.name === "ul" || node.name === "ol");
const items = (list: Element) => list.children.filter((c): c is Element => isTag(c) && c.name === "li");

/// A Checked list with each item marked by what its check found.
function markChecks(node: DOMNode): DOMNode {
  if (!isList(node)) return node;

  const children = node.children.map(child => {
    if (!(isTag(child)) || child.name !== "li") return child;
    const check = checkOf(textOf(child));
    return check ? new Element("li", { ...child.attribs, "data-check": check }, child.children) : child;
  });
  return new Element(node.name, { ...node.attribs }, children);
}

function textOf(node: DOMNode): string {
  if (isText(node)) return node.data;
  if (isTag(node)) return (node.children as DOMNode[]).map(textOf).join("");
  return "";
}

/// The text of `nodes`, its inline code as NULs of the same length, so a
/// period or colon in code ends nothing.
function maskedText(nodes: DOMNode[]): string {
  return nodes.map(node => isTag(node) && node.name === "code" ? "\0".repeat(textOf(node).length) : isTag(node) ? maskedText(node.children as DOMNode[]) : textOf(node)).join("");
}

/// The part of `nodes` from text offset `from` to `to`: whole nodes inside
/// it as they are, nodes it cuts copied with their cut text.
function slice(nodes: DOMNode[], from: number, to: number): DOMNode[] {
  const out: DOMNode[] = [];
  let at = 0;

  for (const node of nodes) {
    const start = at, end = at + textOf(node).length;
    at = end;
    // A node with no text (an image, a break) goes where it stands.
    if (start === end ? start < from || start >= to : end <= from || start >= to) continue;

    if (start >= from && end <= to) out.push(node);
    else if (isText(node)) out.push(new Text(node.data.slice(Math.max(from - start, 0), Math.min(to, end) - start)));
    else if (isTag(node)) out.push(new Element(node.name, { ...node.attribs }, slice(node.children as DOMNode[], from - start, to - start) as Element["children"]));
  }

  return out;
}

/// `nodes` without the whitespace they start with.
function trimStart(nodes: DOMNode[]): DOMNode[] {
  const [first, ...rest] = nodes;
  if (isText(first)) return first.data.trimStart() ? [new Text(first.data.trimStart()), ...rest] : trimStart(rest);
  return nodes;
}

/// `nodes` starting with a capital, as a headline does after "Done: ".
function capitalized(nodes: DOMNode[]): DOMNode[] {
  const [first, ...rest] = nodes;
  return isText(first) ? [new Text(first.data.charAt(0).toUpperCase() + first.data.slice(1)), ...rest] : nodes;
}
