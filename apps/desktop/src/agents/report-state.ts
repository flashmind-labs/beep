// An agent's report, read from its reply's HTML: the reply that ends a
// finished turn, written to Beep's `report` skill (crates/node/skills/report).
// It opens with its outcome (`Done`, `Partly done`, `Blocked`), then the
// headline, then `##` sections the view draws as parts.
import { Element, Text, htmlToDOM, type DOMNode } from "html-react-parser";
import { createStore } from "zustand";

// By type, not class: the parser's CommonJS and ES builds each load domhandler's.
export const isTag = (node: { type: string } | undefined): node is Element => node?.type === "tag" || node?.type === "script" || node?.type === "style";
export const isText = (node: { type: string } | undefined): node is Text => node?.type === "text";

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
  /// A short paragraph the agent wrote before its report ("Two claims broken. Report:").
  lead: DOMNode[];
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
/// when it is neither in the shape nor `minWords` long.
export function reportOf(html: string, text: string, minWords = REPORT_MIN_WORDS): Report | null {
  const { words, minutes } = readingTime(text);
  const nodes = htmlToDOM(html);
  const shaped = readReport(nodes);
  if (shaped) return { kind: "shaped", words, minutes, shaped };
  return words >= minWords ? { kind: "long", words, minutes, nodes } : null;
}

/// What Summarize asks the session's agent about its report `text`: the
/// same report, shorter, naming the report by how it starts.
export function summaryPrompt(text: string) {
  return `${SUMMARY_ASK}"${openingOf(text)}" even further, for a reader with one minute. `
    + "Keep its first line, the outcome and the result. Then at most three one-line bullets: only what I must decide or know before it ships. "
    + "No sections, no files you read, nothing already said.";
}

const SUMMARY_ASK = "Summarize your report that starts ";

/// The opening a Summarize message names, or null for any other message.
export function summaryAsked(prompt: string): string | null {
  if (!prompt.startsWith(SUMMARY_ASK)) return null;
  const quoted = /^"(.*)" even further,/.exec(prompt.slice(SUMMARY_ASK.length));
  return quoted ? quoted[1] : null;
}

/// How a report starts, to name it: its first line without Markdown marks, cut short.
export function openingOf(text: string) {
  const first = text.split("\n").map(line => line.replace(/^#+\s*/, "").replace(/\*\*/g, "").trim()).find(Boolean) ?? "";
  return first.length > 60 ? `${first.slice(0, 60).trimEnd()}…` : first;
}

/// Which reports show their original over the summary Summarize brought, by
/// the summary's id. A report with a summary shows the summary by default.
export const reportViews = createStore<{ original: ReadonlySet<string> }>(() => ({ original: new Set() }));

export function toggleOriginal(id: string) {
  const original = new Set(reportViews.getState().original);
  if (!original.delete(id)) original.add(id);
  reportViews.setState({ original });
}

/// The most words a paragraph before a report's outcome may have.
const LEAD_WORDS = 30;

/// `nodes` read as a report, or null when they don't open with an outcome,
/// or with one short paragraph and then an outcome.
export function readReport(nodes: DOMNode[]): Shaped | null {
  const blocks = nodes.flatMap((node, at) => isText(node) && !node.data.trim() ? [] : [at]);
  const opens = (at: number | undefined) => {
    const node = at === undefined ? undefined : nodes[at];
    return isTag(node) && node.name === "p" ? outcomeOf(maskedText(node.children as DOMNode[])) : null;
  };
  const leads = !opens(blocks[0]) && isTag(nodes[blocks[0]]) && (nodes[blocks[0]] as Element).name === "p"
    && textOf(nodes[blocks[0]]).split(/\s+/).filter(Boolean).length <= LEAD_WORDS && opens(blocks[1]) !== null;
  const start = blocks[leads ? 1 : 0];
  const first = nodes[start];
  if (!(isTag(first)) || first.name !== "p") return null;

  const masked = maskedText(first.children as DOMNode[]);
  const opened = outcomeOf(masked);
  if (!opened) return null;

  const lead = leads ? [nodes[blocks[0]]] : [];

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

  return { outcome: opened.outcome, lead, headline: capitalized(trimStart(headline)), intro, sections };
}

/// The claim `place` names in `shaped`: `Headline`, or the nth item of the
/// section whose title starts with the rest (`Changed 2`, `Needs you 1`);
/// null when there is no such claim.
export function claimAt(shaped: Shaped, place: string): { kind: "headline" } | { kind: "item"; section: number; item: Element } | null {
  if (place.trim().toLowerCase() === "headline") return shaped.headline.length ? { kind: "headline" } : null;

  const named = /^(.+?)\s+(\d+)$/.exec(place.trim());
  if (!named) return null;
  const title = named[1].toLowerCase(), n = Number(named[2]);
  const section = shaped.sections.findIndex(s => s.title.toLowerCase().startsWith(title));
  const item = section < 0 ? undefined : shaped.sections[section].nodes.filter(isList).flatMap(items)[n - 1];
  return item ? { kind: "item", section, item } : null;
}

/// The words a note on claim `place` marks when its own quote isn't in the
/// report: the claim's first sentence, as serpis places a finding.
export function claimWords(shaped: Shaped, place: string): string | null {
  const claim = claimAt(shaped, place);
  if (!claim) return null;

  const text = (claim.kind === "headline" ? shaped.headline.map(textOf).join("") : textOf(claim.item)).replace(/\s+/g, " ").trim();
  const stop = text.indexOf(". ");
  return stop < 0 ? text : text.slice(0, stop + 1);
}

/// `shaped` with the claims `held` names marked `data-held` (the headline
/// as `headlineHeld`), for a challenge that could not break them.
export function withHeld(shaped: Shaped, held: readonly string[]): { sections: ReportSection[]; headlineHeld: boolean } {
  const claims = held.map(place => claimAt(shaped, place));
  const marked = new Set(claims.flatMap(c => c?.kind === "item" ? [c.item] : []));
  const mark = (node: DOMNode): DOMNode => {
    if (!isTag(node)) return node;
    if (marked.has(node)) return new Element(node.name, { ...node.attribs, "data-held": "" }, node.children);
    const children = (node.children as DOMNode[]).map(mark);
    return children.every((child, i) => child === node.children[i]) ? node : new Element(node.name, { ...node.attribs }, children as Element["children"]);
  };

  return {
    sections: marked.size ? shaped.sections.map(s => ({ ...s, nodes: s.nodes.map(mark) })) : shaped.sections,
    headlineHeld: claims.some(c => c?.kind === "headline"),
  };
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

/// The text of `node`, as read.
export function textOf(node: DOMNode): string {
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
