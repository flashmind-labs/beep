// Reviewing what an agent wrote, on screen: Ask and Change over a selection
// in a reply, the comment written in place of the composer, the drafts that
// go with the next message, the drafts under the passages they quote, and
// a sent review drawn as its comments. The state is in review-state.ts.
import { createPortal } from "react-dom";
import { Fragment, useEffect, useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { useStore } from "zustand";
import { useShallow } from "zustand/react/shallow";
import { Button } from "../ui/kit.tsx";
import { AG } from "./agents.ts";
import { Element as DomElement, Text as TextNode, attributesToProps, domToReact, type DOMNode, type HTMLReactParserOptions } from "html-react-parser";
import { isTag, isText, openingOf, textOf } from "./report-state.ts";
import { keepWriting, reviewCount, reviewOf, reviews, updateReview, type FindingOf, type SentNote, type Tag, type Ask, type ReviewComment, type Review, type SentReview } from "./review-state.ts";

const ASK_LABEL: Record<Ask, string> = { question: "Question", change: "Change", finding: "Challenge" };

const TAG_TITLE: Record<Tag, string> = {
  confirmed: "The challenger showed this by running something",
  plausible: "The challenger read this in the code; it did not run anything to show it",
};

/// A comment's heading: what it asks, a finding's tag, and then `rest`.
function CommentHead({ ask, finding, rest }: { ask: Ask; finding: FindingOf | null; rest: string }) {
  return <span className="k"><b>{ASK_LABEL[ask]}</b>{finding && <span className="rv-tag" data-tag={finding.tag} title={TAG_TITLE[finding.tag]}>{finding.tag}</span>}{rest && ` · ${rest}`}</span>;
}

/// A comment's words and a finding's evidence, with their `code` drawn as code.
function CommentWords({ text, finding }: { text: string; finding: FindingOf | null }) {
  return <>
    <span className="t"><Inline text={text} /></span>
    {finding?.evidence && <span className="ev">Evidence: <Inline text={finding.evidence} /></span>}
  </>;
}

/// `text` with each `code` span as code.
function Inline({ text }: { text: string }) {
  return <>{text.split(/`([^`\n]+)`/).map((part, i) => i % 2 ? <code key={i}>{part}</code> : part)}</>;
}

// ----- Starting a comment -----

/// The reply a selection lies in, whole, and the selected range.
function selectedReply(selection: Selection | null) {
  if (!selection || selection.isCollapsed || !selection.rangeCount) return null;

  const range = selection.getRangeAt(0);
  const reply = (node: Node) => (node instanceof Element ? node : node.parentElement)?.closest<HTMLElement>(".dcoderen.agent[data-row] .reply-body")?.closest<HTMLElement>(".dcoderen.agent[data-row]") ?? null;
  const start = reply(range.startContainer);
  if (!start || start !== reply(range.endContainer) || !range.toString().trim()) return null;
  return { reply: start, range };
}

/// Where a node is in a report: `Headline`, `Changed 2` for the second item
/// under Changed, `Changed` elsewhere in that part; "" outside a report's parts.
function placeOf(node: Node) {
  const el = node instanceof Element ? node : node.parentElement;
  if (!el) return "";
  if (el.closest(".report-headline")) return "Headline";

  const part = el.closest<HTMLElement>(".report-part");
  const title = part?.querySelector(":scope > h2")?.firstChild?.textContent?.trim() ?? "";
  if (!part || !title) return "";

  const items = [...part.querySelectorAll(":scope > :is(ul, ol) > li")];
  const item = items.findIndex(li => li.contains(el));
  return item < 0 ? title : `${title} ${item + 1}`;
}

/// Starts a comment on the selection in a reply of the open session; false when there is none.
export function startComment(ask: Ask) {
  const key = reviews.getState().open, selection = getSelection(), at = selectedReply(selection);
  if (!key || !at || reviewOf(key).writing) return false;

  const entry = Number(at.reply.dataset.row), page = AG.page;
  const said = page?.entries[entry - page.earlier];
  const quote = at.range.toString().replace(/[ \t]+\n/g, "\n").trim();
  updateReview(key, review => ({ ...review, writing: { id: null, entry, quote, place: placeOf(at.range.startContainer), opening: openingOf(said?.text ?? ""), ask, text: "", finding: null } }));
  selection?.removeAllRanges();
  return true;
}

/// Ask and Change under a selection in a reply; A and C while it shows.
export function SelectionAsk() {
  const [at, setAt] = useState<DOMRect | null>(null);

  useEffect(() => {
    const place = () => {
      const picked = selectedReply(getSelection());
      const key = reviews.getState().open;
      setAt(picked && key && !reviewOf(key).writing ? picked.range.getBoundingClientRect() : null);
    };
    const onKey = (event: globalThis.KeyboardEvent) => {
      const editing = event.target instanceof HTMLElement && event.target.closest("input, textarea, [contenteditable='true']");
      if (editing || event.metaKey || event.ctrlKey || event.altKey) return;
      const ask: Ask | null = event.key === "a" ? "question" : event.key === "c" ? "change" : null;
      if (ask && startComment(ask)) event.preventDefault();
    };

    document.addEventListener("selectionchange", place);
    window.addEventListener("scroll", place, { capture: true, passive: true });
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("selectionchange", place);
      window.removeEventListener("scroll", place, { capture: true });
      document.removeEventListener("keydown", onKey);
    };
  }, []);

  if (!at) return null;

  const left = Math.min(Math.max(8, at.left + at.width / 2 - 70), window.innerWidth - 150);
  const top = Math.min(at.bottom + 6, window.innerHeight - 40);
  // Pressed without taking the selection away.
  const keep = (event: { preventDefault(): void }) => event.preventDefault();
  return createPortal(<div className="rv-pop" role="toolbar" aria-label="Comment on the selection" style={{ left, top }} onMouseDown={keep}>
    <Button small look="quiet" onClick={() => startComment("question")} title="Ask a question about the selection">Ask<kbd>A</kbd></Button>
    <Button small look="quiet" onClick={() => startComment("change")} title="Ask for a change to the selection">Change<kbd>C</kbd></Button>
  </div>, document.body);
}

// ----- The composer -----

/// Over the composer: the comment being written, which stands in for the
/// field, or the drafts that go with the next message.
export function ReviewField({ reviewKey, sendNow }: { reviewKey: string; sendNow(): void }) {
  const review = useStore(reviews, state => state.by[reviewKey] ?? null);
  if (!review) return null;
  if (review.writing) return <CommentField reviewKey={reviewKey} review={review} sendNow={sendNow} />;
  if (!review.comments.length) return null;

  const set = (change: (r: Review) => Review) => updateReview(reviewKey, change);
  return <div className="rv-drafts">
    {review.listed && <div className="rv-list" role="list" aria-label="Review drafts">{review.comments.map(c => <div key={c.id} role="listitem" className="rv-item" data-ask={c.ask}>
      <button type="button" className="rv-open" onClick={() => set(r => ({ ...r, writing: { ...c } }))} title="Change this comment">
        <span className="k">{ASK_LABEL[c.ask]}</span>
        <span className="w"><i>{c.place ? `${c.place} · ` : ""}“{c.quote}”</i><span>{c.text}</span></span>
      </button>
      <Button small look="quiet" aria-label="Drop this comment" onClick={() => set(r => ({ ...r, comments: r.comments.filter(d => d.id !== c.id) }))}>×</Button>
    </div>)}</div>}
    <div className="rv-bar"><b>Review</b><span>{reviewCount(review.comments)} · goes with your next message</span><span className="sp" />
      <Button small look="quiet" onClick={sendNow} title="Send the review now, without a message">Send now</Button>
      <Button small look="quiet" aria-expanded={review.listed} onClick={() => set(r => ({ ...r, listed: !r.listed }))}>{review.listed ? "Hide" : "Show"}</Button>
      <Button small look="quiet" onClick={() => set(r => ({ ...r, comments: [], listed: false }))}>Discard</Button>
    </div>
  </div>;
}

/// The comment being written: what it is on, the quote, the words. Enter
/// saves it, Esc drops it, ⌘Enter sends the review now.
function CommentField({ reviewKey, review, sendNow }: { reviewKey: string; review: Review; sendNow(): void }) {
  const w = review.writing;
  const field = useRef<HTMLTextAreaElement>(null);
  useLayoutEffect(() => { field.current?.focus(); }, []);
  if (!w) return null;

  const set = (change: (r: Review) => Review) => updateReview(reviewKey, change);
  const save = () => set(keepWriting);
  const cancel = () => set(r => ({ ...r, writing: null }));
  const now = () => { save(); sendNow(); };
  const onKey = (event: KeyboardEvent) => {
    if (event.key === "Escape") { event.preventDefault(); cancel(); }
    else if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) { event.preventDefault(); now(); }
    else if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) { event.preventDefault(); save(); }
  };

  return <div className="rv-field" data-ask={w.ask} role="group" aria-label={`${ASK_LABEL[w.ask]} on the selection`}>
    <div className="h"><b>{ASK_LABEL[w.ask]}</b><span>{w.place ? `on ${w.place}` : "on the selection"}</span><span className="sp" />
      <Button small look="quiet" onClick={cancel}>Cancel<kbd>esc</kbd></Button>
      <Button small onClick={now} disabled={!w.text.trim()} title="Send the review now (⌘⏎)">Send now</Button>
      <Button small look="primary" onClick={save}>Save<kbd>⏎</kbd></Button>
    </div>
    <p className="rv-quote">{w.quote}</p>
    <textarea ref={field} rows={2} value={w.text} aria-label={w.ask === "question" ? "Your question" : w.ask === "finding" ? "What the challenge found" : "The change you ask for"}
      placeholder={w.ask === "question" ? "Ask about this…" : "What should change…"}
      onChange={event => { const text = event.target.value; set(r => r.writing ? { ...r, writing: { ...r.writing, text } } : r); }} onKeyDown={onKey} />
  </div>;
}

// ----- In the transcript -----

const norm = (text: string) => text.replace(/\s+/g, " ").trim();

/// A note under the passage it quotes: a draft, or a comment already sent.
export type Note = { kind: "draft"; comment: ReviewComment } | { kind: "sent"; sent: SentNote };

/// The parts of the reply at `entry` with the comments on it marked: those
/// `sent` on it, the open session's drafts, and the one being written; and
/// where each one's card goes.
/// `claim` gives the words of a claim a note names by its place (`Changed
/// 2`), marked instead when the note's own quote isn't in the reply.
export function useReviewed(entry: string, parts: readonly DOMNode[][], sent: readonly SentNote[], claim: ((place: string) => string | null) | null = null) {
  const drafts = useStore(reviews, useShallow(state => (state.open ? state.by[state.open]?.comments ?? [] : []).filter(c => String(c.entry) === entry)));
  const writing = useStore(reviews, useShallow(state => {
    const w = state.open ? state.by[state.open]?.writing : null;
    return w && String(w.entry) === entry ? { ask: w.ask, quote: w.quote } : null;
  }));

  return useMemo(() => {
    // A quote the reply doesn't hold (a challenger's paraphrase) goes on the claim its place names.
    const said = norm(parts.flat().map(textOf).join(" "));
    const where = (quote: string, place: string) => !claim || said.includes(norm(quote)) ? quote : claim(place) ?? quote;

    // Sent first, so a draft on the same words shows over it.
    const quotes = [
      ...sent.map(n => ({ ask: n.item.ask, quote: where(n.item.quote, n.item.place), sent: true })),
      ...drafts.map(c => ({ ask: c.ask, quote: where(c.quote, c.place), sent: false })),
      ...writing ? [{ ask: writing.ask, quote: writing.quote, sent: false }] : [],
    ];
    const marked = markQuotes(parts, quotes);
    const notes: Note[] = [...sent.map(n => ({ kind: "sent" as const, sent: n })), ...drafts.map(c => ({ kind: "draft" as const, comment: c }))];
    return { parts: marked, ...placeNotes(marked.flat(), notes, note => note.kind === "draft" ? where(note.comment.quote, note.comment.place) : where(note.sent.item.quote, note.sent.item.place)) };
  }, [parts, drafts, writing, sent, claim]);
}

/// `parts` with each quote's words in a `mark` of its kind, a sent one's
/// marked as sent, found with spaces and line breaks counted as one space;
/// quotes not found are left out.
function markQuotes(parts: readonly DOMNode[][], quotes: readonly { ask: Ask; quote: string; sent: boolean }[]): DOMNode[][] {
  if (!quotes.length) return [...parts];

  let flat = "", space = true;
  const at: [TextNode, number][] = [];
  const walk = (nodes: DOMNode[]) => {
    for (const node of nodes) {
      if (isTag(node)) walk(node.children as DOMNode[]);
      if (!isText(node)) continue;

      for (let i = 0; i < node.data.length; i++) {
        const blank = /\s/.test(node.data[i]);
        if (blank && space) continue;
        flat += blank ? " " : node.data[i];
        at.push([node, i]);
        space = blank;
      }
    }
  };
  parts.forEach(walk);

  // Each text node's characters, by the quote they are in, last one first.
  type Kind = { ask: Ask; sent: boolean };
  const kinds = new Map<TextNode, (Kind | null)[]>();
  for (const { ask, quote, sent } of quotes) {
    const wanted = norm(quote), from = wanted ? flat.indexOf(wanted) : -1;
    if (from < 0) continue;

    const spans = new Map<TextNode, [number, number]>();
    for (const [node, i] of at.slice(from, from + wanted.length)) spans.set(node, [spans.get(node)?.[0] ?? i, i + 1]);
    for (const [node, [start, end]] of spans) {
      const list = kinds.get(node) ?? Array<Kind | null>(node.data.length).fill(null);
      list.fill({ ask, sent }, start, end);
      kinds.set(node, list);
    }
  }

  const rebuild = (nodes: DOMNode[]): DOMNode[] => nodes.flatMap(node => {
    if (isTag(node)) {
      const children = rebuild(node.children as DOMNode[]);
      return children.every((child, i) => child === node.children[i]) ? [node] : [new DomElement(node.name, { ...node.attribs }, children as DomElement["children"])];
    }
    const list = isText(node) ? kinds.get(node) : undefined;
    if (!list || !isText(node)) return [node];

    const runs: DOMNode[] = [];
    for (let start = 0, end = 1; start < list.length; start = end, end = start + 1) {
      while (end < list.length && list[end] === list[start]) end++;
      const text = new TextNode(node.data.slice(start, end)), kind = list[start];
      runs.push(kind ? new DomElement("mark", { class: "rv-mark", "data-ask": kind.ask, ...kind.sent ? { "data-sent": "" } : {} }, [text]) : text);
    }
    return runs;
  });
  return parts.map(rebuild);
}

/// Where each note goes: under the list item or top-level block that holds
/// its quote whole, the innermost item first. Notes no block holds go last.
function placeNotes(list: DOMNode[], notes: readonly Note[], quoteOf: (note: Note) => string) {
  const at = new Map<DOMNode, Note[]>(), rest: Note[] = [];
  const place = (note: Note, nodes: DOMNode[], top: boolean): boolean => {
    const node = nodes.find(n => isTag(n) && norm(textOf(n)).includes(norm(quoteOf(note))));
    if (!node || !isTag(node)) return false;
    if (place(note, node.children as DOMNode[], false)) return true;
    if (!top && node.name !== "li") return false;

    at.set(node, [...at.get(node) ?? [], note]);
    return true;
  };

  for (const note of notes) if (!place(note, list, true)) rest.push(note);
  return { at, rest };
}

/// `list` drawn with the notes `at` puts under its blocks and list items.
export function WithNotes({ list, at }: { list: DOMNode[]; at: ReadonlyMap<DOMNode, readonly Note[]> }) {
  const options: HTMLReactParserOptions = {
    replace: node => {
      if (!isTag(node)) return;
      const notes = node.name === "li" ? at.get(node) ?? [] : [], held = "data-held" in node.attribs;
      if (!notes.length && !held) return;

      const Tag = node.name as "li" | "p";
      return <Tag {...attributesToProps(node.attribs)}>{domToReact(node.children as DOMNode[], options)}{held && <HeldChip />}<Notes notes={notes} /></Tag>;
    },
  };

  return <>{list.map((node, i) => <Fragment key={i}>
    {domToReact([node], options)}
    {!(isTag(node) && node.name === "li") && <Notes notes={at.get(node) ?? []} />}
  </Fragment>)}</>;
}

/// Marks a claim a challenge could not break. Its word is drawn by the
/// style, so it stays out of selections and quotes.
const HeldChip = () => <span className="report-held" title="A challenge could not break this claim" />;

/// Notes under the passage they quote: drafts to change or remove, and sent
/// comments, one line each until opened.
export function Notes({ notes }: { notes: readonly Note[] }) {
  return <>{notes.map(note => note.kind === "draft" ? <DraftNote key={`d${note.comment.id}`} c={note.comment} /> : <SentNoteCard key={`s${note.sent.key}`} note={note.sent} />)}</>;
}

/// A draft: a click opens it in the composer again, × removes it.
function DraftNote({ c }: { c: ReviewComment }) {
  const change = (edit: (review: Review) => Review) => {
    const key = reviews.getState().open;
    if (key && !reviewOf(key).writing) updateReview(key, edit);
  };
  return <div className="rv-note" data-ask={c.ask}>
    <button type="button" className="rv-note-open" onClick={() => change(r => ({ ...r, writing: { ...c } }))} title="Change this comment">
      <CommentHead ask={c.ask} finding={c.finding} rest="draft · click to change" /><CommentWords text={c.text} finding={c.finding} />
    </button>
    <Button small look="quiet" className="rv-drop" aria-label="Remove this comment" title="Remove this comment" onClick={() => change(r => ({ ...r, comments: r.comments.filter(d => d.id !== c.id) }))}>×</Button>
  </div>;
}

/// A sent comment: one line until opened, and where it was answered.
function SentNoteCard({ note }: { note: SentNote }) {
  const [open, setOpen] = useState(false);
  const { item, answer } = note;
  return <div className="rv-note sent" data-ask={item.ask} data-open={open || undefined}>
    <button type="button" className="rv-note-open" aria-expanded={open} onClick={() => setOpen(!open)} title={open ? "Show less" : "Show the whole comment"}>
      <CommentHead ask={item.ask} finding={item.finding} rest="sent" /><CommentWords text={item.text} finding={item.finding} />
    </button>
    {answer && <Button small look="quiet" className="rv-answer" onClick={() => showReply(answer)} title="Go to the reply that answered it">answered ↓</Button>}
  </div>;
}

/// Scrolls the log to the reply at entry `row`.
function showReply(row: string) {
  document.querySelector(`.dcoderen.agent[data-row="${row}"]`)?.scrollIntoView({ block: "start", behavior: "smooth" });
}

/// A review as sent: its comments, and what was typed with them.
export function SentReviewCard({ review }: { review: SentReview }) {
  const opening = review.items[0]?.opening;
  return <div className="rv-sent">
    <div className="h"><b>Review</b> · {review.items.length === 1 ? "1 comment" : `${review.items.length} comments`}{opening ? <> on “{opening}”</> : null}</div>
    {review.items.map((item, i) => <div key={i} className="it" data-ask={item.ask}>
      <CommentHead ask={item.ask} finding={item.finding} rest={item.place} />
      <p className="rv-quote">{item.quote}</p>
      <CommentWords text={item.text} finding={item.finding} />
    </div>)}
    {review.also && <div className="also">{review.also}</div>}
  </div>;
}
