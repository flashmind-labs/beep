// Reviewing what an agent wrote, on screen: Ask and Change over a selection
// in a reply, the comment written in place of the composer, the drafts that
// go with the next message, the drafts under the passages they quote, and
// a sent review drawn as its comments. The state is in review-state.ts.
import { createPortal } from "react-dom";
import { useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import { useStore } from "zustand";
import { useShallow } from "zustand/react/shallow";
import { Button } from "../ui/kit.tsx";
import { AG } from "./agents.ts";
import { openingOf } from "./report-state.ts";
import { keepWriting, reviewCount, reviewOf, reviews, updateReview, type Ask, type ReviewComment, type Review, type SentReview } from "./review-state.ts";

const ASK_LABEL: Record<Ask, string> = { question: "Question", change: "Change" };

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
  updateReview(key, review => ({ ...review, writing: { id: null, entry, quote, place: placeOf(at.range.startContainer), opening: openingOf(said?.text ?? ""), ask, text: "" } }));
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
    <textarea ref={field} rows={2} value={w.text} aria-label={w.ask === "question" ? "Your question" : "The change you ask for"}
      placeholder={w.ask === "question" ? "Ask about this…" : "What should change…"}
      onChange={event => { const text = event.target.value; set(r => r.writing ? { ...r, writing: { ...r.writing, text } } : r); }} onKeyDown={onKey} />
  </div>;
}

// ----- In the transcript -----

const norm = (text: string) => text.replace(/\s+/g, " ").trim();

/// The open session's drafts on the reply at `entry`.
export function useDrafts(entry: string) {
  return useStore(reviews, useShallow(state => (state.open ? state.by[state.open]?.comments ?? [] : []).filter(c => String(c.entry) === entry)));
}

/// The drafts of `drafts` whose quote is in `text`.
export const draftsIn = (drafts: readonly ReviewComment[], text: string) => drafts.filter(c => norm(text).includes(norm(c.quote)));

/// Drafts under the passage they quote; a click opens one in the composer again.
export function DraftNotes({ drafts }: { drafts: readonly ReviewComment[] }) {
  if (!drafts.length) return null;

  const open = (c: ReviewComment) => {
    const key = reviews.getState().open;
    if (key && !reviewOf(key).writing) updateReview(key, r => ({ ...r, writing: { ...c } }));
  };
  return <>{drafts.map(c => <button type="button" key={c.id} className="rv-note" data-ask={c.ask} onClick={() => open(c)} title="Change this comment">
    <span className="k"><b>{ASK_LABEL[c.ask]}</b> · draft</span><span className="t">{c.text}</span>
  </button>)}</>;
}

/// Marks the passages the open session's drafts quote, and the one being
/// written, without changing the replies' markup.
export function paintReviewMarks() {
  if (typeof CSS === "undefined" || !("highlights" in CSS)) return;

  const state = reviews.getState(), review = state.open ? state.by[state.open] : undefined;
  const marks: Record<Ask, Range[]> = { question: [], change: [] };
  for (const c of [...review?.comments ?? [], ...review?.writing ? [review.writing] : []]) {
    const body = document.querySelector(`.dcoderen.agent[data-row="${c.entry}"] .reply-body`);
    const range = body && rangeOf(body, c.quote);
    if (range) marks[c.ask].push(range);
  }

  CSS.highlights.set("review-question", new Highlight(...marks.question));
  CSS.highlights.set("review-change", new Highlight(...marks.change));
}

if (typeof requestAnimationFrame === "function") reviews.subscribe(() => requestAnimationFrame(paintReviewMarks));

/// Where `quote` is in `root`'s text, spaces and line breaks counted as one space.
function rangeOf(root: Element, quote: string) {
  const wanted = norm(quote);
  if (!wanted) return null;

  const at: [Text, number][] = [];
  let flat = "", space = true;
  const walk = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  for (let node = walk.nextNode(); node instanceof Text; node = walk.nextNode()) {
    for (let i = 0; i < node.data.length; i++) {
      const blank = /\s/.test(node.data[i]);
      if (blank && space) continue;
      flat += blank ? " " : node.data[i];
      at.push([node, i]);
      space = blank;
    }
  }

  const from = flat.indexOf(wanted);
  if (from < 0) return null;

  const range = document.createRange(), [start, offset] = at[from], [end, last] = at[from + wanted.length - 1];
  range.setStart(start, offset);
  range.setEnd(end, last + 1);
  return range;
}

/// A review as sent: its comments, and what was typed with them.
export function SentReviewCard({ review }: { review: SentReview }) {
  const opening = review.items[0]?.opening;
  return <div className="rv-sent">
    <div className="h"><b>Review</b> · {review.items.length === 1 ? "1 comment" : `${review.items.length} comments`}{opening ? <> on “{opening}”</> : null}</div>
    {review.items.map((item, i) => <div key={i} className="it" data-ask={item.ask}>
      <span className="k"><b>{ASK_LABEL[item.ask]}</b>{item.place ? ` · ${item.place}` : ""}</span>
      <p className="rv-quote">{item.quote}</p>
      <span className="t">{item.text}</span>
    </div>)}
    {review.also && <div className="also">{review.also}</div>}
  </div>;
}
