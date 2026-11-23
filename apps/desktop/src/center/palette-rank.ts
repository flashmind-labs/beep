// Ranks what was typed against the palette's index, in memory: a tier per
// match, the best few of each group, and one top hit. The app builds the
// index (palette/index.rs), so a keystroke never leaves this window.

export type HitKind = "action" | "setting" | "code" | "branch" | "project" | "folder" | "agent_session" | "machine";

/// One thing the palette can find (Entry).
export interface Entry<S = unknown> {
  kind: HitKind;
  id: string;
  title: string;
  detail: string;
  snippet?: string;
  /// The lowercase texts it is found by.
  keys: string[];
  /// The session an agent_session entry opens.
  session?: S;
  /// The project or folder an agent_session entry runs in, by that entry's id.
  project?: string;
}

/// An entry that was found.
export interface Hit<S = unknown> extends Entry<S> {
  /// The one best hit, listed first under Top hit.
  top?: boolean;
}

/// Everything the palette finds, a list per group (Index).
export interface PaletteIndex<S = unknown> {
  actions: Entry<S>[];
  settings: Entry<S>[];
  code: Entry<S>[];
  projects: Entry<S>[];
  sessions: Entry<S>[];
  machines: Entry<S>[];
}

/// Most hits of one group.
const PER_GROUP = 8;

/// How well a text matches, best first.
const PREFIX = 0, WORD_START = 1, SUBSTRING = 2, TEXT = 3, NO_MATCH = 4;

/// Which group's best hit becomes the top hit on a tie: the things a search
/// most often means first.
const TOP_ORDER: HitKind[] = ["project", "folder", "machine", "code", "branch", "agent_session", "setting", "action"];

const WORD_CHAR = /[\p{L}\p{N}]/u;

/// `query` and `text` are lowercase.
function tier(query: string, text: string): number {
  let at = text.indexOf(query);
  if (at < 0) return NO_MATCH;
  if (at === 0) return PREFIX;

  for (; at >= 0; at = text.indexOf(query, at + 1)) {
    if (!WORD_CHAR.test(text[at - 1])) return WORD_START;
  }

  return SUBSTRING;
}

interface Ranked<S> {
  entry: Entry<S>;
  tier: number;
  titleMatches: number;
}

function titleMatches(query: string, title: string): number {
  const text = title.toLowerCase();
  let count = 0;

  for (let at = text.indexOf(query); at >= 0; at = text.indexOf(query, at + query.length)) {
    count++;
  }

  return count;
}

/// Within a tier, prefer more occurrences in the displayed title. Aliases
/// can qualify a result without counting the same visible match twice.
function matches<S>(query: string, group: Entry<S>[]): Ranked<S>[] {
  const hits: Ranked<S>[] = [];

  for (const entry of group) {
    let best = NO_MATCH;
    for (const key of entry.keys) best = Math.min(best, tier(query, key));
    if (best !== NO_MATCH) hits.push({ entry, tier: best, titleMatches: titleMatches(query, entry.title) });
  }

  return hits.sort((a, b) => a.tier - b.tier || b.titleMatches - a.titleMatches);
}

/// A session found by title and prompt is one hit, with its matching excerpt.
function withText<S>(sessions: Ranked<S>[], text: Entry<S>[]) {
  for (const found of text) {
    if (found.kind !== "agent_session") continue;
    const match = sessions.find(hit => hit.entry.id === found.id);

    if (match) {
      match.entry = { ...match.entry, detail: found.detail, snippet: found.snippet };
    } else {
      sessions.push({ entry: found, tier: TEXT, titleMatches: 0 });
    }
  }
}

/// The hits for `query`, a group at a time; `text` holds the coding sessions
/// whose prompts hold it, when they are known. Nothing typed lists every action.
export function rank<S>(rawQuery: string, index: PaletteIndex<S>, text: Entry<S>[] = []): Hit<S>[] {
  const query = rawQuery.trim().toLowerCase();
  if (!query) return [...index.actions];
  const sessions = matches(query, index.sessions);
  withText(sessions, text);

  const find = (group: Entry<S>[]) => matches(query, group);
  const groups = [find(index.projects), find(index.machines), sessions, find(index.code), find(index.settings), find(index.actions)];
  const top = topHit(groups);
  const out: Hit<S>[] = top ? [{ ...top.entry, top: true }] : [];

  for (const group of groups) {
    for (const hit of group.slice(0, PER_GROUP)) {
      if (hit !== top) out.push(hit.entry);
    }
  }

  return out;
}

/// With several matching groups, promote the strongest prefix match.
function topHit<S>(groups: Ranked<S>[][]): Ranked<S> | null {
  if (groups.filter(group => group.length).length < 2) return null;

  const candidates = groups.map(group => group[0]).filter(hit => hit?.tier === PREFIX);
  candidates.sort((a, b) => b.titleMatches - a.titleMatches || TOP_ORDER.indexOf(a.entry.kind) - TOP_ORDER.indexOf(b.entry.kind));
  return candidates[0] ?? null;
}
