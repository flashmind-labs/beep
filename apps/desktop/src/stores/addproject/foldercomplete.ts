// Folder completion for the Import a project sheet's Folder field: what to
// list for the text typed, which of its folders to offer, and what the
// menu's keys do. The sheet reads the folders with `list_machine_folder`.
// The folder picker's path and filter are here too.

/// One step of the picker's path: an ancestor to go back to.
export interface Crumb {
  label: string;
  path: string;
}

/// The way down to `path` as steps, the folder itself last: from `Home` when
/// it is under the machine's `home`, else from `/`.
export function crumbs(path: string, home: string | null): Crumb[] {
  const under = home && home !== "/" && (path === home || path.startsWith(`${home}/`));
  const from: Crumb = under ? { label: "Home", path: home } : { label: "/", path: "/" };
  const steps = [from];

  for (const name of path.slice(from.path.length).split("/").filter(Boolean)) {
    steps.push({ label: name, path: `${steps[steps.length - 1].path.replace(/\/$/, "")}/${name}` });
  }

  return steps;
}

/// The folders whose name holds `filter`, ignoring case, those starting with
/// it first.
export function matching(folders: readonly string[], filter: string): string[] {
  const want = filter.trim().toLowerCase();
  if (!want) return [...folders];
  const found = folders.filter(name => name.toLowerCase().includes(want));
  const starts = (name: string) => name.toLowerCase().startsWith(want);

  return [...found.filter(starts), ...found.filter(name => !starts(name))];
}

/// The most folders the menu offers.
const MAX_SUGGESTIONS = 12;

/// Typed text split at its last `/`: the folder to list, as typed with its
/// trailing `/` (`/`, `/srv/code/`, `~/`), and the start of a name in it.
interface TypedFolder {
  dir: string;
  prefix: string;
}

/// Where to look for `text`: null when it is not a path the machine can
/// read, that is neither absolute nor under `~/`, or has a `..` or a
/// control character in it.
export function splitTyped(text: string): TypedFolder | null {
  if (!text.startsWith("/") && !text.startsWith("~/")) return null;
  if (/[\u0000-\u001f\u007f]/.test(text)) return null;
  const cut = text.lastIndexOf("/");
  const dir = text.slice(0, cut + 1);
  if (dir.split("/").includes("..")) return null;
  return { dir, prefix: text.slice(cut + 1) };
}

/// The folders of `folders` that start with `prefix`, ignoring case, by name
/// and at most `cap` of them.
export function suggest(folders: readonly string[], prefix: string, cap = MAX_SUGGESTIONS): string[] {
  const start = prefix.toLowerCase();
  const hidden = prefix.startsWith(".");
  const found = folders
    .filter(name => (hidden || !name.startsWith(".")) && name.toLowerCase().startsWith(start))
    .sort((a, b) => {
      const x = a.toLowerCase(), y = b.toLowerCase();
      return x < y ? -1 : x > y ? 1 : a < b ? -1 : a > b ? 1 : 0;
    })
    .slice(0, cap);
  return found.length === 1 && found[0] === prefix ? [] : found;
}

/// The text after picking `name` in `dir`: the folder with a `/`, so its
/// own folders come next.
export const completed = (dir: string, name: string) => `${dir}${name}/`;

/// The menu under the field: the folders offered in `dir`, and the one
/// highlighted.
export interface FolderMenu {
  dir: string;
  items: readonly string[];
  index: number;
}

/// No menu.
export const NO_MENU: FolderMenu = { dir: "", items: [], index: 0 };

/// A menu of `items` in `dir`, the first highlighted.
export const menuOf = (dir: string, items: readonly string[]): FolderMenu => ({ dir, items, index: 0 });

export const isOpen = (menu: FolderMenu) => menu.items.length > 0;

/// What a key does to the menu:
/// - `move` highlights another row (Up and Down, wrapping)
/// - `accept` picks the highlighted folder (Tab or Enter)
/// - `close` shuts the menu (Escape)
/// - `none` leaves the key to the field, as every key does with no menu
export type MenuAction =
  | { type: "none" }
  | { type: "move"; index: number }
  | { type: "accept"; text: string }
  | { type: "close" };

/// What `key` does with `menu` open; `modified` when Shift, Alt, Ctrl or
/// Cmd is held, which leaves every key to the field.
export function menuKey(menu: FolderMenu, key: string, modified = false): MenuAction {
  const n = menu.items.length;
  if (!n || modified) return { type: "none" };
  const at = Math.min(Math.max(menu.index, 0), n - 1);

  switch (key) {
    case "ArrowDown": return { type: "move", index: (at + 1) % n };
    case "ArrowUp": return { type: "move", index: (at - 1 + n) % n };
    case "Tab":
    case "Enter": return { type: "accept", text: completed(menu.dir, menu.items[at]) };
    case "Escape": return { type: "close" };
    default: return { type: "none" };
  }
}
