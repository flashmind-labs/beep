// Where the window's Settings sheet stands: the page picked in its left
// pane and the subpage open under it, such as Apps then Gmail. Pure, so
// the tests can drive it; bar/settings.ts draws it.

/// The pages of Settings, in the order the left pane lists them.
export const SETTINGS_PAGES = [
  ["general", "General"],
  ["appearance", "Appearance"],
  ["models", "Models"],
  ["apps", "Apps"],
  ["mac", "This Mac"],
  ["machines", "Machines"],
  ["agents", "Agent accounts"],
  ["creds", "Credentials"],
  ["instr", "Instructions"],
  ["devices", "Account"],
] as const;

export type SettingsPage = (typeof SETTINGS_PAGES)[number][0];

/// A subpage, by what it shows: an app's permissions, an MCP server's
/// tools, a machine, or a coding agent's settings.
export type SettingsSub =
  | { kind: "app"; id: string }
  | { kind: "mcp"; id: string }
  | { kind: "machine"; id: string }
  | { kind: "agent"; id: string };

/// The coding agents Settings lists under Agent accounts, in order. Each
/// opens its own subpage, nested in the left pane like Apps and Machines.
export const CODING_AGENTS = ["claude", "codex", "gemini", "pi", "opencode", "flsh"] as const;

/// The subpages Agent accounts nests: one per coding agent.
export function agentSubs(): SettingsSub[] {
  return CODING_AGENTS.map(id => ({ kind: "agent", id }));
}

/// Which page each kind of subpage sits under.
const SUB_PAGE: Record<SettingsSub["kind"], SettingsPage> = { app: "apps", mcp: "apps", machine: "machines", agent: "agents" };

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

/// `path`, or its page when the subpage is gone, or the first page shown
/// when its page is not (such as Models on a server that offers none).
/// `subExists` says whether a subpage still has something to show.
export function fitPath(path: SettingsPath, shown: readonly SettingsPage[], subExists: (sub: SettingsSub) => boolean = () => true): SettingsPath {
  if (!shown.includes(path.page)) return { page: shown[0] ?? "general", sub: null };
  if (path.sub && !subExists(path.sub)) return { page: path.page, sub: null };
  return path;
}

/// A subpage as one string: `app:gmail`.
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

/// The path as one string, for storage: `apps/app:gmail`.
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
export const REMEMBER_KEY = "beep.settings.page";

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

/// Keeps `path` for the next time Settings opens. Storage that throws
/// only means the next open starts on General.
export function rememberPath(store: Store | null | undefined, path: SettingsPath) {
  try {
    store?.setItem(REMEMBER_KEY, pathKey(path));
  } catch {
    // Nothing kept: the next open starts on General.
  }
}

/// The breadcrumb atop the right pane: the page, then the subpage when one
/// is open. `subLabel` names a subpage, such as an app's name.
export function crumbs(path: SettingsPath, subLabel: (sub: SettingsSub) => string): string[] {
  const page = pageLabel(path.page);
  return path.sub ? [page, subLabel(path.sub)] : [page];
}

/// One entry of the left pane: a page, or a subpage nested under it.
export interface NavEntry {
  page: SettingsPage;
  sub: SettingsSub | null;
  /// The entry open in the right pane.
  current: boolean;
}

/// The left pane: each page shown, and under the page picked the subpages
/// `subs` lists for it, such as each coding agent under Agent accounts.
export function navEntries(shown: readonly SettingsPage[], path: SettingsPath, subs: (page: SettingsPage) => readonly SettingsSub[]): NavEntry[] {
  return shown.flatMap(page => {
    const here = path.page === page;
    const own: NavEntry = { page, sub: null, current: here && !path.sub };
    if (!here) return [own];
    const kids = subs(page).map(sub => ({ page, sub, current: !!path.sub && path.sub.kind === sub.kind && path.sub.id === sub.id }));
    return [own, ...kids];
  });
}
