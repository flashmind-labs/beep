// Ranks what was typed against the palette's index, in memory: a tier per
// match, the best few of each group, and one top hit. The app builds the
// index (palette/index.rs), so a keystroke never leaves this window.

import { agentInProject, type AgentLike } from "../../stores/project/projectwindow.ts";

export type HitKind = "action" | "setting" | "code" | "branch" | "pr" | "issue" | "terminal" | "project" | "folder" | "agent_session" | "machine";

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
  prs?: Entry<S>[];
  issues?: Entry<S>[];
  terminals?: Entry<S>[];
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
const TOP_ORDER: HitKind[] = ["project", "folder", "machine", "pr", "issue", "code", "branch", "agent_session", "terminal", "setting", "action"];

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
  const groups = [
    find(index.projects),
    find(index.machines),
    sessions,
    find(index.prs ?? []),
    find(index.issues ?? []),
    find(index.code),
    find(index.terminals ?? []),
    find(index.settings),
    find(index.actions),
  ];
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

/// What a detached project window gives Command K so it searches only that project.
export interface ProjectPaletteScope<S extends AgentLike = AgentLike> {
  project: string;
  label: string;
  imported: boolean;
  board: boolean;
  hasTeam: boolean;
  hasCheckout: boolean;
  folders: readonly { id: string; folder: string }[];
  prs?: readonly { number: number; title: string; author: string; mine: boolean; draft: boolean; labels?: readonly string[] }[] | null;
  issues?: readonly { number: number; title: string; author: string; labels: readonly string[]; assignees: readonly string[] }[] | null;
  terminals?: readonly { id: string; name: string; command: string; machine_name: string; state: string }[] | null;
  sessions?: readonly S[] | null;
}

const HARNESS_LABELS: Record<string, string> = {
  claude: "Claude Code",
  codex: "Codex",
  agy: "Antigravity",
  pi: "pi",
  flsh: "flsh",
  opencode: "OpenCode",
  hermes: "Hermes",
};

function loweredKeys(values: readonly (string | null | undefined)[]): string[] {
  const out: string[] = [];

  for (const value of values) {
    const trimmed = value?.trim().toLowerCase();
    if (trimmed && !out.includes(trimmed)) out.push(trimmed);
  }

  return out;
}

function actionEntry<S>(id: string, title: string, keys: readonly string[]): Entry<S> {
  return { kind: "action", id, title, detail: "", keys: loweredKeys([title, ...keys]) };
}

function projectActions<S extends AgentLike>(scope: ProjectPaletteScope<S>): Entry<S>[] {
  const actions: Entry<S>[] = [
    actionEntry("new_session", "New session", ["start an agent", "new agent"]),
  ];
  if (scope.hasCheckout) actions.push(actionEntry("new_terminal", "New terminal", ["terminal", "shell"]));
  actions.push(actionEntry("project_overview", scope.board ? "Board" : "Overview", ["overview", "board"]));

  if (scope.imported) {
    actions.push(
      actionEntry("project_prs", "Pull requests", ["prs", "reviews"]),
      actionEntry("project_issues", "Issues", ["bugs"]),
      actionEntry("project_branches", "Git", ["branches", "history", "worktrees"]),
      actionEntry("project_actions", "Actions", ["runs", "ci", "workflows", "builds"]),
      actionEntry("project_sessions", "Sessions", ["past sessions", "custom agents"]),
      actionEntry("project_machines", "Machines", ["sync", "checkouts", "folders"]),
    );
    if (scope.hasTeam) actions.push(actionEntry("project_team", "Team", ["members"]));
  }

  actions.push(
    actionEntry("project_skills", "Skills", ["project skills"]),
    actionEntry("project_harnesses", "Harnesses", ["coding agents", "allowed agents"]),
  );
  return actions;
}

function sessionMatchesScope<S extends AgentLike>(entry: Entry<S>, scope: ProjectPaletteScope<S>): boolean {
  if (entry.project !== undefined) {
    return entry.project === scope.project && (!entry.session || agentInProject(entry.session, scope.folders));
  }

  return !!entry.session && scope.folders.length > 0 && agentInProject(entry.session, scope.folders);
}

function scopedSessionEntry<S extends AgentLike>(entry: Entry<S>, project: string): Entry<S> {
  const projectKey = project.toLowerCase();
  const titleKey = entry.title.trim().toLowerCase();
  const keys = entry.keys.filter(key => key !== projectKey || titleKey === projectKey);
  const detail = entry.detail === project ? (entry.session?.machine_name ?? "") : entry.detail;
  return { ...entry, detail, keys };
}

function sessionFromRow<S extends AgentLike>(row: S, project: string): Entry<S> {
  const harness = HARNESS_LABELS[row.agent ?? ""] || row.agent || "Agent";
  const title = row.title?.trim() || harness;
  const id = `${row.machine_id} ${row.id ?? ""}`.trim();
  return {
    kind: "agent_session",
    id,
    title,
    detail: row.machine_name ?? "",
    keys: loweredKeys([title, harness, row.agent, row.machine_name]),
    session: row,
    project,
  };
}

/// Prompt search entries filtered and formatted for `scope`.
export function projectPromptEntries<S extends AgentLike>(entries: readonly Entry<S>[], scope: ProjectPaletteScope<S>): Entry<S>[] {
  return entries.filter(entry => sessionMatchesScope(entry, scope)).map(entry => scopedSessionEntry(entry, scope.project));
}

/// The palette index focused on `scope`: only that project's actions, sessions,
/// pull requests, issues, branches and terminals.
export function projectPaletteIndex<S extends AgentLike>(global: PaletteIndex<S>, scope: ProjectPaletteScope<S>): PaletteIndex<S> {
  const prefix = `${scope.project} `;
  const titlePrefix = `${scope.project} · `;

  const code: Entry<S>[] = global.code
    .filter(entry => entry.kind === "branch" && entry.id.startsWith(prefix))
    .map(entry => {
      const name = entry.title.startsWith(titlePrefix) ? entry.title.slice(titlePrefix.length) : entry.id.slice(prefix.length);
      return { ...entry, title: name, keys: loweredKeys([name, entry.detail]) };
    });

  const sessions = global.sessions
    .filter(entry => sessionMatchesScope(entry, scope))
    .map(entry => scopedSessionEntry(entry, scope.project));
  const seen = new Set(sessions.map(entry => entry.id));

  for (const row of scope.sessions ?? []) {
    if (!agentInProject(row, scope.folders)) continue;
    const entry = sessionFromRow(row, scope.project);
    if (seen.has(entry.id)) continue;
    seen.add(entry.id);
    sessions.push(entry);
  }

  sessions.sort((a, b) => (b.session?.last_activity ?? 0) - (a.session?.last_activity ?? 0));

  const prs: Entry<S>[] = (scope.prs ?? []).map(pr => {
    const author = pr.mine ? "you" : pr.author;
    return {
      kind: "pr",
      id: String(pr.number),
      title: `#${pr.number} ${pr.title}`,
      detail: [author, pr.draft ? "draft" : ""].filter(Boolean).join(" · "),
      keys: loweredKeys([`#${pr.number}`, String(pr.number), pr.title, author, pr.author, ...(pr.labels ?? [])]),
    };
  });

  const issues: Entry<S>[] = (scope.issues ?? []).map(issue => {
    return {
      kind: "issue",
      id: String(issue.number),
      title: `#${issue.number} ${issue.title}`,
      detail: issue.assignees.length ? issue.assignees.join(", ") : (issue.author ? `by ${issue.author}` : ""),
      keys: loweredKeys([`#${issue.number}`, String(issue.number), issue.title, issue.author, ...issue.labels, ...issue.assignees]),
    };
  });

  const terminals: Entry<S>[] = (scope.terminals ?? []).map(term => {
    const title = term.name || term.command || "Terminal";
    return {
      kind: "terminal",
      id: term.id,
      title,
      detail: [term.machine_name, term.state !== "running" ? term.state : ""].filter(Boolean).join(" · "),
      keys: loweredKeys([term.name, term.command, term.machine_name]),
    };
  });

  return {
    actions: projectActions(scope),
    settings: [],
    code,
    prs,
    issues,
    terminals,
    projects: [],
    sessions,
    machines: [],
  };
}

export function projectPalettePlaceholder(label: string): string {
  return `Search ${label}: sessions, PRs, issues, branches…`;
}
