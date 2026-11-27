// Where the window's Settings sheet stands: the page picked in its left pane
// and the subpage open under it, such as Machines then one machine.

/// The pages of Settings, in the order the left pane lists them, with the
/// words its search also finds them by.
const SETTINGS_PAGES = [
  ["general", "General", "name updates version"],
  ["appearance", "Appearance", "skin theme colors paintings motion dark light system"],
  ["mac", "This computer", "control screen recording accessibility"],
  ["machines", "Machines", "nodes devices"],
  ["agents", "Agents", "accounts login sign in models coding agents"],
  ["assistant", "Assistant", ""],
  ["creds", "Credentials", "tokens keys api github forgejo gitea secrets"],
  ["instr", "Instructions", "agents.md claude.md prompt rules"],
  ["skills", "Skills", ""],
  ["plugins", "Plugins", "extensions"],
] as const;

export type SettingsPage = (typeof SETTINGS_PAGES)[number][0];

export type SettingsGroup = "App" | "Fleet" | "Agents" | "Extensions";

/// Which group each Settings page sits under in the left navigation pane.
export function pageGroup(page: SettingsPage): SettingsGroup {
  switch (page) {
    case "general":
    case "appearance":
      return "App";
    case "mac":
    case "machines":
      return "Fleet";
    case "agents":
    case "assistant":
    case "creds":
      return "Agents";
    case "instr":
    case "skills":
    case "plugins":
      return "Extensions";
  }
}

/// The rows search finds by title, under the page that shows them, titled
/// as the page titles them.
export const SETTING_ROWS: readonly (readonly [SettingsPage, string])[] = [
  ["general", "Launch at login"],
  ["general", "Coding agent notifications"],
  ["general", "Voice dictation"],
  ["general", "Sync coding agent skills"],
  ["general", "Project sync"],
  ["general", "Save files to"],
  ["general", "Keyboard shortcuts"],
  ["general", "Developer mode"],
  ["general", "Report a problem"],
  ["appearance", "System theme"],
  ["appearance", "Projects"],
  ["appearance", "Conversation scrollbar"],
  ["appearance", "Prompt marks"],
  ["mac", "Let Beep use this computer"],
  ["assistant", "Harness"],
];

/// A subpage, by what it shows: a machine, or a coding agent's settings.
export type SettingsSub =
  | { kind: "machine"; id: string }
  | { kind: "agent"; id: string };

/// The coding agents Settings lists under Agent accounts, in order. Each
/// opens its own subpage, nested in the left pane like Machines.
export const CODING_AGENTS = ["claude", "codex", "agy", "pi", "opencode", "flsh"] as const;

/// Which page each kind of subpage sits under.
const SUB_PAGE: Record<SettingsSub["kind"], SettingsPage> = { machine: "machines", agent: "agents" };

export interface SettingsPath {
  page: SettingsPage;
  sub: SettingsSub | null;
}

export const FIRST_PAGE: SettingsPath = { page: "general", sub: null };

export function isPage(id: unknown): id is SettingsPage {
  return SETTINGS_PAGES.some(([p]) => p === id);
}

export function pageLabel(page: SettingsPage) {
  return SETTINGS_PAGES.find(([p]) => p === page)?.[1] ?? page;
}

/// The page picked in the left pane; any subpage closes.
export function selectPage(page: SettingsPage): SettingsPath {
  return { page, sub: null };
}

/// Opens `sub` under the page it belongs to.
export function openSub(sub: SettingsSub): SettingsPath {
  return { page: SUB_PAGE[sub.kind], sub };
}

/// Back from a subpage to its page; null when already on a page.
export function back(path: SettingsPath): SettingsPath | null {
  return path.sub ? { page: path.page, sub: null } : null;
}

export function samePath(a: SettingsPath, b: SettingsPath) {
  return a.page === b.page && a.sub?.kind === b.sub?.kind && a.sub?.id === b.sub?.id;
}

/// `path`, or its page when the subpage is gone, or the first page shown when
/// its page is not (such as This computer on a phone).
export function fitPath(path: SettingsPath, shown: readonly SettingsPage[], subExists: (sub: SettingsSub) => boolean = () => true): SettingsPath {
  if (!shown.includes(path.page)) return { page: shown[0] ?? "general", sub: null };
  if (path.sub && !subExists(path.sub)) return { page: path.page, sub: null };
  return path;
}

/// A subpage as one string: `machine:abc`.
export function subKey(sub: SettingsSub) {
  return `${sub.kind}:${sub.id}`;
}

/// Reads a key `subKey` wrote; null when it names no kind of subpage.
export function parseSub(key: string | null | undefined): SettingsSub | null {
  const colon = key?.indexOf(":") ?? -1;
  if (!key || colon < 1) return null;
  const kind = key.slice(0, colon), id = key.slice(colon + 1);
  if (!id || !Object.hasOwn(SUB_PAGE, kind)) return null;
  return { kind, id } as SettingsSub;
}

/// The path as one string, for storage: `machines/machine:abc`.
export function pathKey(path: SettingsPath) {
  return path.sub ? `${path.page}/${subKey(path.sub)}` : path.page;
}

/// Reads a key `pathKey` wrote; null when it does not name a page.
export function parsePath(key: string | null | undefined): SettingsPath | null {
  if (!key) return null;
  const slash = key.indexOf("/");
  const page = slash < 0 ? key : key.slice(0, slash);
  if (!isPage(page)) return null;
  const sub = slash < 0 ? null : parseSub(key.slice(slash + 1));
  // A subpage stored under another page is not trusted.
  return sub && SUB_PAGE[sub.kind] === page ? { page, sub } : { page, sub: null };
}

/// Where the last picked page is kept, per Mac.
const REMEMBER_KEY = "beep.settings.page";

type Store = Pick<Storage, "getItem" | "setItem">;

/// The page Settings opens on: the one picked last, else General. Storage
/// that throws (a private window) reads as nothing kept.
export function recallPath(store: Store | null | undefined): SettingsPath {
  try {
    return parsePath(store?.getItem(REMEMBER_KEY)) ?? FIRST_PAGE;
  } catch {
    return FIRST_PAGE;
  }
}

/// Keeps `path` for the next time Settings opens.
export function rememberPath(store: Store | null | undefined, path: SettingsPath) {
  try {
    store?.setItem(REMEMBER_KEY, pathKey(path));
  } catch {
    // Nothing kept: the next open starts on General.
  }
}

function wordsMatch(text: string, query: string) {
  const words = text.toLowerCase().split(/\s+/);
  return query.toLowerCase().split(/\s+/).filter(Boolean).every(q => words.some(w => w.startsWith(q)));
}

/// The left pane while searching. First what `query` finds by name: pages,
/// subpages such as Codex under Agent accounts, and rows such as Developer
/// mode under General. Then the pages it finds only by their search words.
/// Each word of `query` starts a word of what it finds.
export function searchEntries(shown: readonly SettingsPage[], path: SettingsPath, query: string, subs: (page: SettingsPage) => readonly SettingsSub[], subName: (sub: SettingsSub) => string): NavEntry[] {
  const page = (page: SettingsPage): NavEntry => ({ page, sub: null, setting: null, current: path.page === page && !path.sub });
  const named = shown.flatMap(p => {
    const own = wordsMatch(pageLabel(p), query) ? [page(p)] : [];
    const kids = subs(p).filter(sub => wordsMatch(subName(sub), query))
      .map(sub => ({ page: p, sub, setting: null, current: path.sub?.kind === sub.kind && path.sub.id === sub.id }));
    return [...own, ...kids];
  });
  const rows = SETTING_ROWS.filter(([p, title]) => shown.includes(p) && wordsMatch(title, query))
    .map(([p, setting]) => ({ page: p, sub: null, setting, current: false }));
  const byWords = SETTINGS_PAGES.filter(([p, label, words]) => shown.includes(p) && !wordsMatch(label, query) && wordsMatch(`${label} ${words}`, query)).map(([p]) => page(p));

  return [...named, ...rows, ...byWords];
}

/// One entry of the left pane: a page, a subpage nested under it, or, while
/// searching, a row of the page.
export interface NavEntry {
  page: SettingsPage;
  sub: SettingsSub | null;
  /// The title of the row this entry points at.
  setting: string | null;
  /// The entry open in the right pane.
  current: boolean;
}

/// The left pane: each page shown, and under the page picked the subpages
/// `subs` lists for it, such as each machine under Machines.
export function navEntries(shown: readonly SettingsPage[], path: SettingsPath, subs: (page: SettingsPage) => readonly SettingsSub[]): NavEntry[] {
  return shown.flatMap(page => {
    const here = path.page === page;
    const kids = here ? subs(page).map(sub => ({ page, sub, setting: null, current: !!path.sub && path.sub.kind === sub.kind && path.sub.id === sub.id })) : [];
    const own: NavEntry = { page, sub: null, setting: null, current: here && (!path.sub || !kids.length) };
    return [own, ...kids];
  });
}
