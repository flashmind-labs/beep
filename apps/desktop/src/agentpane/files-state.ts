// The Files tab of a coding agent session: the working folder's tree as
// the node lists it a few levels at a time, what of it shows for a filter,
// paths mentioned in a message, files dropped to upload, and the simple
// highlighting of the text view. No page here, so the tests load it.
import { esc } from "../ui/format.ts";
import { extension } from "./media-state.ts";

/// How git sees an entry changed (WorkEntry.change).
export type GitChange = "untracked" | "added" | "modified" | "conflicted";

/// A file or folder of a listing (TreeEntry).
export interface TreeEntry {
  name: string;
  folder: boolean;
  size: number;
  ignored: boolean;
  change: GitChange | null;
}

/// One folder as `agent_tree` gives it (TreeFolder).
export interface TreeFolder {
  path: string;
  entries: TreeEntry[];
  more: number;
  git: boolean;
}

/// A read of the tree as `agent_tree` gives it (Tree): the folder asked
/// first, then those under it the node read.
export interface Tree {
  folders: TreeFolder[];
  truncated: boolean;
}

/// A folder's listing in one string, to tell a read that changed nothing.
export function listingKey(l: Listing | undefined) {
  if (!l) return "";
  const rows = l.entries ? l.entries.map(e => `${e.name}\t${+e.folder}\t${e.size}\t${+e.ignored}\t${e.change ?? ""}`).join("\n") : "-";
  return `${l.error}\n${l.more}\n${rows}`;
}

/// Puts what `tree` read into `listings`, over what they held. True when
/// a listing differs from the one it replaces.
export function mergeTree(listings: Map<string, Listing>, tree: Tree) {
  let changed = false;
  for (const f of tree.folders) {
    const next: Listing = { entries: f.entries, more: f.more, error: "" };
    if (listingKey(listings.get(f.path)) !== listingKey(next)) changed = true;
    listings.set(f.path, next);
  }
  return changed;
}

/// `dir` is not read, or a folder in it is not: opening it reads on. The
/// node does not read into what git ignores, so that does not count.
export function atEdge(listings: ReadonlyMap<string, Listing>, dir: string) {
  const entries = listings.get(dir)?.entries;
  return !entries || entries.some(e => e.folder && !e.ignored && !listings.get(joinPath(dir, e.name))?.entries);
}

/// Folders named in `listings` and not read yet, for a filter to read. The
/// node does not read into what git ignores, so those stay out.
export function unreadFolders(listings: ReadonlyMap<string, Listing>) {
  const out: string[] = [];
  for (const [dir, listing] of listings) {
    for (const e of listing.entries ?? []) {
      const path = joinPath(dir, e.name);
      if (e.folder && !e.ignored && !listings.has(path)) out.push(path);
    }
  }
  return out;
}

/// A folder's listing as the tab holds it: loading, read, or why not.
export interface Listing {
  entries: TreeEntry[] | null;
  more: number;
  error: string;
}

/// A folder's listing after `agent_tree` failed with `text`. A folder the
/// machine no longer has, such as a temporary one, reads as empty: no error.
export function failedListing(text: string, had?: Listing): Listing {
  const msg = text.replace(/^bad request: /, "");
  if (/: the folder .* is not there/.test(msg)) return { entries: [], more: 0, error: "" };
  const error = /unknown op|not serve|update/i.test(msg) ? "Update Beep's server and beep-node on that machine to browse its files." : msg;
  return { entries: had?.entries ?? null, more: 0, error };
}

/// A row of the tree on screen.
export interface TreeRow {
  path: string;
  name: string;
  depth: number;
  folder: boolean;
  open: boolean;
  size: number;
  /// It or a folder above it is ignored.
  ignored: boolean;
  change: GitChange | null;
  /// A folder open but not read yet, or that could not be.
  loading: boolean;
  error: string;
  /// Entries of an open folder left out past the node's limit.
  more: number;
}

/// `name` inside the folder `dir` ("" for the root).
export function joinPath(dir: string, name: string) {
  const d = dir.replace(/\/+$/, "");
  return d ? `${d}/${name}` : name;
}

/// The folder a path is in, "" for the root.
export function parentPath(path: string) {
  const at = path.lastIndexOf("/");
  return at < 0 ? "" : path.slice(0, at);
}

/// The rows the tree shows: open folders' entries under them, depth first.
export function treeRows(listings: ReadonlyMap<string, Listing>, open: ReadonlySet<string>, filter = ""): TreeRow[] {
  const want = filter.trim().toLowerCase();
  const out: TreeRow[] = [];
  const walk = (dir: string, depth: number, hidden: boolean): boolean => {
    const listing = listings.get(dir);
    let any = false;
    for (const e of listing?.entries ?? []) {
      const path = joinPath(dir, e.name);
      const isOpen = e.folder && (open.has(path) || (!!want && listings.has(path)));
      const sub = listings.get(path);
      const row: TreeRow = {
        path, name: e.name, depth, folder: e.folder, open: isOpen, size: e.size,
        ignored: hidden || e.ignored, change: e.change,
        loading: isOpen && !!sub && sub.entries === null && !sub.error, error: isOpen ? sub?.error ?? "" : "", more: isOpen ? sub?.more ?? 0 : 0,
      };
      const at = out.length;
      out.push(row);
      const under = isOpen ? walk(path, depth + 1, row.ignored) : false;
      const hit = !want || e.name.toLowerCase().includes(want) || under;
      if (!hit) out.length = at;
      any ||= hit;
    }
    return any;
  };
  walk("", 0, false);
  return out;
}

/// The paths of a message's mentions as the agent reads them: `@path`,
/// quoted when it has a space.
export function mentionText(paths: readonly string[]) {
  return paths.map(p => (/\s/.test(p) ? `@"${p}"` : `@${p}`)).join(" ");
}

/// The text a message sends: what was typed, then its mentions.
export function withMentions(text: string, paths: readonly string[]) {
  return [text.trim(), mentionText(paths)].filter(Boolean).join("\n\n");
}

/// Where dropped files go unless a folder is picked.
export const UPLOAD_FOLDER = ".beep/uploads";
/// The largest file a drop uploads, as the node takes it.
export const MAX_UPLOAD = 64 * 1024 * 1024;
/// The bytes one upload part carries.
export const UPLOAD_PART = 2 * 1024 * 1024;

/// The parts a file of `size` bytes goes in: their offsets and lengths.
/// An empty file is one empty part, so it is still made.
export function uploadParts(size: number, part = UPLOAD_PART) {
  const out: { offset: number; len: number }[] = [];
  let offset = 0;
  do {
    const len = Math.min(part, size - offset);
    out.push({ offset, len });
    offset += len;
  } while (offset < size);
  return out;
}

/// Why a dropped file cannot be uploaded, or "" when it can.
export function uploadRefusal(f: { name: string; size: number }) {
  return f.size > MAX_UPLOAD ? `${f.name || "That file"} is over ${MAX_UPLOAD >> 20} MB, the most a drop uploads` : "";
}

/// A file's size in short words.
export function shortSize(n: number) {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${Math.round(n / 102.4) / 10} KB`;
  return `${Math.round(n / (1024 * 104.8576)) / 10} MB`;
}

// ----- The text view -----

/// The most lines the text view draws.
export const MAX_LINES = 20000;

const LANGS: Record<string, string> = {
  rs: "Rust", ts: "TypeScript", tsx: "TypeScript", js: "JavaScript", mjs: "JavaScript", cjs: "JavaScript", jsx: "JavaScript",
  py: "Python", go: "Go", swift: "Swift", kt: "Kotlin", java: "Java", c: "C", h: "C", cc: "C++", cpp: "C++", hpp: "C++",
  rb: "Ruby", sh: "Shell", bash: "Shell", zsh: "Shell", toml: "TOML", json: "JSON", yaml: "YAML", yml: "YAML",
  md: "Markdown", html: "HTML", css: "CSS", sql: "SQL", lua: "Lua", txt: "Text",
};

/// The language a file is in by its extension, or "".
export function languageOf(path: string) {
  return LANGS[extension(path)] ?? "";
}

const KEYWORDS = new Set(("as async await break case catch class const continue crate def default defer del do elif else enum export extends "
  + "false fn for from func function go if impl import in interface is let loop match mod module mut new nil None null package pass "
  + "private pub raise return self Self static struct super switch this throw trait true True False try type typeof use var where while with yield").split(" "));
const HASH_COMMENT = new Set(["Python", "Shell", "Ruby", "TOML", "YAML"]);

/// One line as HTML, its strings, comments, numbers and keywords in spans (`s`,
/// `c`, `n`, `k`).
export function highlight(line: string, lang: string) {
  if (!lang || lang === "Text" || lang === "Markdown") return esc(line);
  const hash = HASH_COMMENT.has(lang);
  const re = hash
    ? /(#.*$)|("(?:[^"\\]|\\.)*"?|'(?:[^'\\]|\\.)*'?)|(\b\d[\d_.xXa-fA-F]*\b)|([A-Za-z_][A-Za-z0-9_]*)/g
    : /(\/\/.*$|\/\*.*?(?:\*\/|$))|("(?:[^"\\]|\\.)*"?|`(?:[^`\\]|\\.)*`?)|(\b\d[\d_.xXa-fA-F]*\b)|([A-Za-z_][A-Za-z0-9_]*)/g;
  let out = "", last = 0;
  for (const m of line.matchAll(re)) {
    out += esc(line.slice(last, m.index));
    const [all, comment, str, num, word] = m;
    const cls = comment ? "c" : str ? "s" : num ? "n" : word && KEYWORDS.has(word) ? "k" : "";
    out += cls ? `<span class="${cls}">${esc(all)}</span>` : esc(all);
    last = m.index! + all.length;
  }
  return out + esc(line.slice(last));
}

// ----- Terminals in the pane -----

/// The name of a new terminal tab: the folder's name and the first number
/// not taken by another tab of that folder.
export function terminalName(cwd: string, taken: readonly string[]) {
  const base = cwd.replace(/\/+$/, "").split("/").pop() || "shell";
  for (let n = 1; ; n++) {
    const name = `${base} ${n}`;
    if (!taken.includes(name)) return name;
  }
}
