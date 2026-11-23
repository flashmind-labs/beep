// Reviewing what an agent wrote: questions and changes on quoted parts of its
// replies, kept as drafts per session and sent as one message with what the
// user types next. The message reads back from the transcript as a review,
// so the chat shows its comments rather than the prompt Beep wrote.
import { createStore } from "zustand";

/// What a comment asks for.
export type Ask = "question" | "change";

/// A draft on part of an agent's reply.
export interface ReviewComment {
  id: number;
  /// The transcript entry of the reply it is about.
  entry: number;
  /// The words it is about, as selected.
  quote: string;
  /// Where they are in a report: `Headline`, `Changed 2`, `Needs you`; "" elsewhere.
  place: string;
  /// How the reply starts, so the agent knows which one.
  opening: string;
  ask: Ask;
  text: string;
}

/// A comment being written: a new one (`id` null) or a saved one again.
export type Writing = Omit<ReviewComment, "id"> & { id: number | null };

/// A session's review: its drafts, the one being written, and whether the list shows.
export interface Review {
  comments: readonly ReviewComment[];
  writing: Writing | null;
  listed: boolean;
}

const EMPTY: Review = { comments: [], writing: null, listed: false };

/// Every session's review, by `machine:session`, and the session shown.
export const reviews = createStore<{ open: string | null; by: Readonly<Record<string, Review>> }>(() => ({ open: null, by: {} }));

export const reviewKey = (machine: string, session: string) => `${machine}:${session}`;

/// The review of `key`, read from this machine's storage the first time.
export function reviewOf(key: string | null): Review {
  if (key === null) return EMPTY;
  const kept = reviews.getState().by[key];
  if (kept) return kept;

  const loaded = { ...EMPTY, comments: loadComments(key) };
  reviews.setState(state => ({ by: { ...state.by, [key]: loaded } }));
  return loaded;
}

export function showReview(key: string | null) {
  if (key !== null) reviewOf(key);
  if (reviews.getState().open !== key) reviews.setState({ open: key });
}

/// Changes the review of `key`, keeping its drafts on this machine.
export function updateReview(key: string, change: (review: Review) => Review) {
  const before = reviewOf(key), after = change(before);
  reviews.setState(state => ({ by: { ...state.by, [key]: after } }));
  if (after.comments !== before.comments) saveComments(key, after.comments);
}

/// Saves the comment being written: a new draft, or the one it changes.
/// Empty words drop it.
export function keepWriting(review: Review): Review {
  const w = review.writing;
  if (!w) return review;

  const others = review.comments.filter(c => c.id !== w.id);
  if (!w.text.trim()) return { ...review, writing: null, comments: w.id === null ? review.comments : others };

  const id = w.id ?? Math.max(0, ...review.comments.map(c => c.id)) + 1;
  const kept: ReviewComment = { ...w, id, text: w.text.trim() };
  const comments = w.id === null ? [...review.comments, kept] : review.comments.map(c => c.id === id ? kept : c);
  return { ...review, writing: null, comments };
}

/// "1 question · 2 changes".
export function reviewCount(comments: readonly ReviewComment[]) {
  const questions = comments.filter(c => c.ask === "question").length, changes = comments.length - questions;
  return [questions && `${questions} ${questions === 1 ? "question" : "questions"}`, changes && `${changes} ${changes === 1 ? "change" : "changes"}`].filter(Boolean).join(" · ");
}

// ----- The message -----

const HEAD = "Review comments on what you wrote.";
const REPLY = "Reply with a report, as after any task. Each change asked for is a bullet under `## Changed`. Each answer comes after the report's first line, quoting what it answers. A place such as item 2 under \"Changed\" is in your last report: don't use it as a heading.";
const SECTIONS: Record<Ask, { title: string; how: string }> = {
  question: { title: "Questions", how: "Answer each, quoting it. Change nothing for these." },
  change: { title: "Changes asked for", how: "Make these changes. When one is unclear or wrong, say so instead of guessing." },
};
const ALSO = "Also";

/// The message the agent gets for `comments`, with what the user typed (`also`) last.
export function reviewPrompt(comments: readonly ReviewComment[], also: string) {
  let out = `${HEAD}\n\n${REPLY}`;

  for (const ask of ["question", "change"] as const) {
    const of = comments.filter(c => c.ask === ask);
    if (!of.length) continue;

    out += `\n\n## ${SECTIONS[ask].title}\n${SECTIONS[ask].how}\n`;
    for (const c of of) {
      const at = placeWords(c.place);
      out += `\nIn your message that starts "${c.opening}"${at ? `, ${at}` : ""}:\n`;
      out += c.quote.split("\n").map(line => `> ${line.trimEnd()}`).join("\n");
      out += `\n${c.text.trim()}\n`;
    }
  }

  if (also.trim()) out += `\n\n## ${ALSO}\n\n${also.trim()}\n`;
  return out;
}

/// A review as sent, read back from the transcript.
export interface SentReview {
  items: { ask: Ask; place: string; opening: string; quote: string; text: string }[];
  also: string;
}

/// `text` read as a review Beep sent, or null for any other message.
export function parseReview(text: string): SentReview | null {
  if (!text.trimStart().startsWith(HEAD)) return null;

  const out: SentReview = { items: [], also: "" };
  let ask: Ask | null = null, also: string[] | null = null;
  let item: SentReview["items"][number] | null = null;
  let quote: string[] = [], words: string[] = [];
  const finish = () => {
    if (item) out.items.push({ ...item, quote: quote.join("\n"), text: words.join("\n").trim() });
    item = null;
    quote = [];
    words = [];
  };

  for (const line of text.split("\n")) {
    if (also) { also.push(line); continue; }

    const heading = /^## (.+)$/.exec(line);
    if (heading) {
      finish();
      const title = heading[1].trim();
      ask = title === SECTIONS.question.title ? "question" : title === SECTIONS.change.title ? "change" : null;
      if (title === ALSO) also = [];
      continue;
    }
    if (!ask) continue;

    const start = /^In your message that starts "(.*?)"(?:, (.+))?:$/.exec(line);
    if (start) {
      finish();
      item = { ask, place: labelOf(start[2] ?? ""), opening: start[1], quote: "", text: "" };
      continue;
    }
    if (!item) continue;

    if (line.startsWith("> ") && !words.length) quote.push(line.slice(2));
    else words.push(line);
  }

  finish();
  out.also = also ? also.join("\n").trim() : "";
  return out.items.length ? out : null;
}

/// A place in words, so it reads as a place in the last report and not a
/// heading to reuse: `Changed 2` is item 2 under "Changed".
export function placeWords(place: string) {
  const p = place.trim();
  if (!p) return "";
  if (p === "Headline") return "its first line";

  const numbered = /^(.+) (\d+)$/.exec(p);
  return numbered ? `item ${numbered[2]} under "${numbered[1]}"` : `under "${p}"`;
}

/// The place `placeWords` wrote.
function labelOf(words: string) {
  if (words === "its first line") return "Headline";
  const numbered = /^item (\d+) under "(.+)"$/.exec(words);
  if (numbered) return `${numbered[2]} ${numbered[1]}`;
  return /^under "(.+)"$/.exec(words)?.[1] ?? words;
}

// ----- Kept on this machine -----

const storageKey = (key: string) => `beep.chatReview.${key}`;

function loadComments(key: string): ReviewComment[] {
  try {
    const raw: unknown = JSON.parse(localStorage.getItem(storageKey(key)) ?? "[]");
    return Array.isArray(raw) ? raw.filter(isComment) : [];
  } catch {
    return [];
  }
}

function saveComments(key: string, comments: readonly ReviewComment[]) {
  try {
    if (comments.length) localStorage.setItem(storageKey(key), JSON.stringify(comments));
    else localStorage.removeItem(storageKey(key));
  } catch { /* kept for this launch only */ }
}

function isComment(value: unknown): value is ReviewComment {
  return typeof value === "object" && value !== null
    && "id" in value && typeof value.id === "number" && "entry" in value && typeof value.entry === "number"
    && "quote" in value && typeof value.quote === "string" && "place" in value && typeof value.place === "string"
    && "opening" in value && typeof value.opening === "string" && "ask" in value && (value.ask === "question" || value.ask === "change")
    && "text" in value && typeof value.text === "string";
}
