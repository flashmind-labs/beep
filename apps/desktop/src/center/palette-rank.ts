// Ranks what was typed against the palette's index, in memory: a tier per
// match, the best few of each group, and one top hit. The app builds the
// index (palette/index.rs), so a keystroke never leaves this window.

export type HitKind = "action" | "setting" | "code" | "branch" | "project" | "project_chat" | "folder" | "agent_session" | "chat" | "memory" | "routine" | "machine";

/// One thing the palette can find (Entry).
export interface Entry<S = unknown> {
  kind: HitKind;
  id: string;
  title: string;
  detail: string;
  /// The lowercase texts it is found by.
  keys: string[];
  /// The session an agent_session entry opens.
  session?: S;
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
  /// Newest first.
  chats: Entry<S>[];
  memories: Entry<S>[];
  routines: Entry<S>[];
  machines: Entry<S>[];
}

/// Most hits of one group.
const PER_GROUP = 8;
/// Chats listed before anything is typed.
const RECENT_CHATS = 5;

/// How well a text matches, best first.
const PREFIX = 0, WORD_START = 1, SUBSTRING = 2, TEXT = 3, TIERS = 4;
const NO_MATCH = TIERS;

/// Which group's best hit becomes the top hit on a tie: the things a search
/// most often means first.
const TOP_ORDER: HitKind[] = ["project", "project_chat", "folder", "machine", "code", "branch", "agent_session", "setting", "chat", "routine", "memory", "action"];

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

/// One group's matches by tier, ties in the group's order.
function matches<S>(query: string, group: Entry<S>[]): Hit<S>[][] {
  const tiers: Hit<S>[][] = Array.from({ length: TIERS }, () => []);
  for (const entry of group) {
    let best = NO_MATCH;
    for (const key of entry.keys) best = Math.min(best, tier(query, key));
    if (best !== NO_MATCH) tiers[best].push(entry);
  }
  return tiers;
}

/// Chats found by title and by text are one hit, with the first snippet.
function withText<S>(tiers: Hit<S>[][], text: Entry<S>[]) {
  for (const found of text) {
    let known = false;
    for (const hits of tiers) {
      const at = hits.findIndex(hit => hit.id === found.id);
      if (at < 0) continue;
      known = true;
      if (!hits[at].detail) hits[at] = { ...hits[at], detail: found.detail };
    }
    if (!known) tiers[TEXT].push(found);
  }
}

/// The hits for `query`, a group at a time; `text` holds the chats whose
/// messages hold it, when they are known. Nothing typed lists every action
/// and the newest chats.
export function rank<S>(rawQuery: string, index: PaletteIndex<S>, text: Entry<S>[] = []): Hit<S>[] {
  const query = rawQuery.trim().toLowerCase();
  if (!query) return [...index.actions, ...index.chats.slice(0, RECENT_CHATS)];
  const chats = matches(query, index.chats);
  withText(chats, text);
  const find = (group: Entry<S>[]) => matches(query, group);
  const groups = [
    find(index.actions), find(index.settings), find(index.code), find(index.projects), find(index.sessions),
    chats, find(index.memories), find(index.routines), find(index.machines),
  ];
  const shown = groups.map(tiers => tiers.flat().slice(0, PER_GROUP));
  const top = topGroup(groups, shown);
  const out: Hit<S>[] = top < 0 ? [] : [{ ...shown[top][0], top: true }];
  shown.forEach((hits, i) => out.push(...(i === top ? hits.slice(1) : hits)));
  return out;
}

/// The group of the top hit, -1 for none: the best hit that starts with what
/// was typed, when hits come from more than one group.
function topGroup<S>(groups: Hit<S>[][][], shown: Hit<S>[][]): number {
  if (shown.filter(hits => hits.length).length < 2) return -1;
  for (const kind of TOP_ORDER) {
    const at = groups.findIndex((tiers, i) => tiers[PREFIX].length && shown[i][0].kind === kind);
    if (at >= 0) return at;
  }
  return -1;
}
