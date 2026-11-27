// The Files tab of a coding agent session: the working folder's tree as
// the node lists it a few levels at a time, what of it shows for a filter,
// paths mentioned in a message, files dropped to upload, and the simple
// highlighting of the text view. No page here, so the tests load it.
import { esc } from "../../ui/format.ts";
import { type FileRef, extension, linkedPath, mediaKind } from "./media-state.ts";

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
interface TreeFolder {
  path: string;
  entries: TreeEntry[];
  more: number;
}

/// A read of the tree as `agent_tree` gives it (Tree): the folder asked
/// first, then those under it the node read.
export interface Tree {
  folders: TreeFolder[];
  truncated: boolean;
}

/// Whether a reply to `agent_tree` is a tree to read.
export function isTree(value: unknown): value is Tree {
  return typeof value === "object" && value !== null && "folders" in value && Array.isArray(value.folders);
}

/// A folder's listing in one string, to tell a read that changed nothing.
function listingKey(l: Listing | undefined) {
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
  const error = /unknown op|not serve|update/i.test(msg) ? "Update beep-node on that machine to browse its files." : msg;
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

/// Parsed file filter: `/` separates ordered path segments (a trailing `/`
/// requires a folder), and a plain term matches a single file or folder name.
interface TreeFilter {
  segments: string[];
  byPath: boolean;
  dirOnly: boolean;
}

function parseFilter(filter: string): TreeFilter | null {
  const raw = filter.trim().toLowerCase().replace(/\\+/g, "/").replace(/\/+/g, "/").replace(/^\.\//, "").replace(/^\//, "");
  if (!raw) return null;
  const dirOnly = raw.endsWith("/");
  const segments = raw.split("/").filter(Boolean);
  if (!segments.length) return null;
  return { segments, byPath: raw.includes("/"), dirOnly };
}

/// The rows the tree shows: open folders' entries under them, depth first, but for the top
/// folders in `left`, such as a chat's threads' own.
export function treeRows(listings: ReadonlyMap<string, Listing>, open: ReadonlySet<string>, filter = "", left: ReadonlySet<string> = new Set()): TreeRow[] {
  const query = parseFilter(filter);
  const out: TreeRow[] = [];
  const walk = (dir: string, depth: number, hidden: boolean, matched: number): boolean => {
    const listing = listings.get(dir);
    let any = false;

    for (const e of listing?.entries ?? []) {
      const path = joinPath(dir, e.name);
      if (!dir && e.folder && left.has(e.name)) continue;
      const isOpen = e.folder && (open.has(path) || (!!query && listings.has(path)));
      const sub = listings.get(path);
      let nextMatched = matched;
      let selfHit = !query;

      if (query) {
        const lower = e.name.toLowerCase();
        if (!query.byPath) {
          selfHit = lower.includes(query.segments[0]);
        } else if (matched >= query.segments.length) {
          selfHit = true;
        } else {
          const isLast = matched === query.segments.length - 1;
          if ((!isLast || !query.dirOnly || e.folder) && lower.includes(query.segments[matched])) {
            nextMatched = matched + 1;
            selfHit = nextMatched === query.segments.length;
          }
        }
      }

      const row: TreeRow = {
        path, name: e.name, depth, folder: e.folder, open: isOpen, size: e.size,
        ignored: hidden || e.ignored, change: e.change,
        loading: isOpen && !!sub && sub.entries === null && !sub.error, error: isOpen ? sub?.error ?? "" : "", more: isOpen ? sub?.more ?? 0 : 0,
      };
      const at = out.length;
      out.push(row);
      const under = isOpen ? walk(path, depth + 1, row.ignored, nextMatched) : false;
      const hit = selfHit || under;
      if (!hit) out.length = at;
      else if (query && e.folder && !under && !row.loading && !row.error && !row.more) row.open = false;
      any ||= hit;
    }

    return any;
  };
  walk("", 0, false, 0);
  return out;
}

/// The paths of a message's mentions as the agent reads them: `@path`,
/// quoted when it has a space.
export function mentionText(paths: readonly string[]) {
  return paths.map(p => (/\s/.test(p) ? `@"${p}"` : `@${p}`)).join(" ");
}

/// The text a message sends: what was typed, then its mentions. Image-only
/// messages get a request before the filenames, which are not the task.
export function withMentions(text: string, paths: readonly string[], images = 0) {
  const count = Math.max(images, paths.length);
  const request = text.trim() || (count > 0 && paths.every(path => mediaKind(path) === "image")
    ? count === 1 ? "Check this image." : "Check these images."
    : "");

  return [request, mentionText(paths)].filter(Boolean).join("\n\n");
}

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

/// A file's size in short words.
export function shortSize(n: number) {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${Math.round(n / 102.4) / 10} KB`;
  return `${Math.round(n / (1024 * 104.8576)) / 10} MB`;
}

/// A download or upload under way: when it started, in ms, and its bytes.
export interface Transfer { started: number; done: number; size: number }

/// "4.2 MB/s" for a transfer slow enough to want it: on for over five
/// seconds and under 80% done. Null otherwise.
export function transferSpeed({ started, done, size }: Transfer, now = Date.now()) {
  const seconds = (now - started) / 1000;
  if (seconds <= 5 || !size || done >= size * 0.8) return null;

  return `${shortSize(Math.round(done / seconds))}/s`;
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

/// The file an inline code span in a reply names, such as `src/main.rs:12`:
/// one word ending in an extension, with a folder, a line, or an extension
/// the pane knows, so `self.mcp` stays code.
export function codeFile(text: string): FileRef | null {
  if (!/^(?:~|\.{1,2})?\/?[\w@+-][\w@+.\/-]*\.[a-z][a-z0-9]*(?::\d+(?::\d+)?|#L\d+(?:-L?\d+)?)?$/i.test(text)) return null;
  const ref = linkedPath(text);
  if (!ref) return null;
  return ref.path.includes("/") || ref.line !== null || languageOf(ref.path) || mediaKind(ref.path) ? ref : null;
}

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
