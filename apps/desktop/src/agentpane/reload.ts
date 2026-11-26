// Live reload of the pane's file tabs and the picked artifact. The node says
// when something in a watched folder changed; the files on screen in it are
// then hashed on their machine, one ask per folder, at most every half second
// while the pane shows, and read again only when a hash changed. A tab out
// of sight is checked when it shows again. `reload-state.ts` says what a
// stamp is.
import { ambient, paneShown, single } from "../ui/active.ts";
import { invoke } from "../ui/tauri.ts";
import { artifactVersion } from "./artifacts-state.ts";
import { followedArtifact, reloadArtifact } from "./artifacts.ts";
import { isTree, type Tree } from "./files-state.ts";
import { isHtmlFile } from "./html-preview-state.ts";
import { type MediaTab, type TreeText, P, paneAsks } from "./pane-state.ts";
import { touchTab } from "./pane-tabs.ts";
import { resolvePath } from "./media-state.ts";
import { dockOf, openTabs, paneModel } from "./media-layout.ts";
import { load } from "./read.ts";
import { type Followed, type Stamp, followedAt, folderStamp, hashedStamp, listedSize, nextStamp, splitPath, textStamp } from "./reload-state.ts";
import { refreshPreview, takeText } from "./text-view.tsx";
import { touches } from "./touched.ts";
import { paneRoot, watchRoot } from "./worktree.ts";

/// A file followed, how it is checked, and what reads it again.
interface Follow extends Followed {
  key: string;
  machine: string;
  /// How a node that cannot hash is asked: the file's size in its folder's
  /// listing, all its text, or for an HTML page its text and the files beside
  /// and under it.
  fallback: "size" | "text" | "page";
  /// The URL an HTML page shows at: the files it loaded count as its own.
  src: string | null;
  shown: boolean;
  /// Takes the change; false when it cannot now, as over unsaved edits.
  reload: () => Promise<boolean>;
}

/// A followed file's stamp, null until read, whether a change was heard in its folder since it
/// was checked, and the files its page loaded when it was last stamped.
interface Held { stamp: Stamp | null; stale: boolean; loaded: string[] }

const held = new Map<string, Held>();
/// The watch of each machine's folder that a followed file is in.
const watches = new Map<string, () => void>();
/// The most paths one ask hashes, as the node takes them (`MAX_HASH_PATHS`).
const HASH_PATHS = 64;

// The tabs ask for the files they show to be followed.
paneAsks.subscribe((now, was) => { if (now.follows !== was.follows) followFiles(); });

/// Follows the files the pane's tabs and picked artifact show now, as the
/// pane draws: a new one is stamped, a watch nothing needs goes, and a tab
/// that shows again after a change is checked.
export function followFiles() {
  const follows = followed();
  const keys = new Set(follows.map(f => f.key));
  for (const key of held.keys()) if (!keys.has(key)) held.delete(key);

  const added = follows.filter(f => !held.has(f.key));
  const first = added.map((f): [string, Held] => [f.key, { stamp: null, stale: false, loaded: [] }]);
  for (const [key, h] of first) held.set(key, h);
  if (added.length) void stampAll(added).then(stamps => { for (const [key, h] of first) h.stamp ??= stamps.get(key) ?? null; });

  watchFolders(follows);
  if (follows.some(f => f.shown && held.get(f.key)?.stale)) check();
}

/// Lets go of every watch as the pane goes.
export function unfollowFiles() {
  for (const stop of watches.values()) stop();
  watches.clear();
  held.clear();
}

/// Checks the files on screen whose folder changed, one check at a time.
const check = ambient(single(async () => {
  const due = followed().flatMap(f => {
    const h = held.get(f.key);
    if (!h?.stale || !f.shown) return [];
    h.stale = false;
    return [{ f, h }];
  });
  const stamps = await stampAll(due.map(({ f }) => f));

  await Promise.all(due.map(async ({ f, h }) => {
    const now = stamps.get(f.key);
    if (!now || held.get(f.key) !== h) return;

    if (!h.stamp) {
      h.stamp = now;
      return;
    }

    const next = nextStamp(h.stamp, now);
    if (!next.changed || await f.reload()) h.stamp = next.stamp;
    else h.stale = true;
  }));
}), 500, paneShown);

function followed(): Follow[] {
  const s = P.session;
  if (!s) return [];
  const machine = s.machine_id, roots = [paneRoot(), s.cwd];

  const tabs = openTabs().flatMap((t): Follow[] => {
    if (t.error) return [];
    const shown = !t.viewHidden && !paneModel.getState().layout.hidden[dockOf(t.id)];

    if (t.kind === "text" && t.rel) {
      const root = t.root ?? s.cwd, path = t.rel;
      return [{ key: `${t.id}\n${root}\n${path}`, machine, root, path, fallback: isHtmlFile(path) ? "page" : "text", src: t.preview?.src || null, shown, reload: () => reloadText(t, { machine, root, path }) }];
    }

    if (t.kind === "term" || !t.path) return [];
    return [{ key: `${t.id}\n${t.path}`, machine, ...followedAt(roots, t.path), fallback: "size", src: null, shown, reload: async () => reloadMedia(t) }];
  });

  const picked = followedArtifact();
  if (!picked || picked.artifact.kind === "pdf") return tabs;
  const a = picked.artifact;

  return [...tabs, {
    key: `artifact\n${a.path}\n${artifactVersion(a)}`, machine, ...followedAt([s.cwd], a.path),
    fallback: a.kind === "image" ? "size" : a.kind === "html" ? "page" : "text", src: picked.src, shown: picked.shown,
    reload: async () => { reloadArtifact(); return true; },
  }];
}

/// A text tab takes its file as it is now, unless it holds unsaved edits:
/// a save then finds the change and asks.
async function reloadText(t: MediaTab, at: { machine: string; root: string; path: string }) {
  if (t.edit?.dirty) return false;
  const now = await invoke<TreeText>("agent_tree_text", { machineId: at.machine, root: at.root, path: at.path }).catch(() => null);
  if (!now || t.edit?.dirty || !openTabs().includes(t)) return false;

  if (t.text?.text !== now.text || t.text.size !== now.size) {
    takeText(t, now);
    touchTab(t);
  }

  return refreshPreview(t);
}

/// A media tab reads its file again once its first read is done.
function reloadMedia(t: MediaTab) {
  if (!t.url) return false;
  void load(t, true);
  return true;
}

/// A page can live inside a worktree or build folder skipped by the project
/// watch. Keep the project watch for assets the page loads from elsewhere.
function watchedRoots(f: Follow): string[] {
  const dir = f.fallback === "page" ? splitPath(f.path).dir : "";
  return dir ? [f.root, resolvePath(f.root, dir)] : [f.root];
}

function watchFolders(follows: Follow[]) {
  const wanted = new Map<string, { machine: string; root: string }>();
  for (const f of follows) {
    for (const root of watchedRoots(f)) wanted.set(`${f.machine}\n${root}`, { machine: f.machine, root });
  }

  for (const [key, stop] of watches) {
    if (wanted.has(key)) continue;
    stop();
    watches.delete(key);
  }

  for (const [key, { machine, root }] of wanted) {
    if (!watches.has(key)) watches.set(key, watchRoot(machine, root, (_, paths) => heard(machine, root, paths)));
  }
}

/// Something in `root` changed, at `paths` when the node says: the followed files it concerns
/// are checked.
function heard(machine: string, root: string, paths: string[]) {
  let any = false;

  for (const f of followed()) {
    const h = held.get(f.key);

    if (h && f.machine === machine && watchedRoots(f).includes(root) && concerns(f, h, root, paths)) {
      h.stale = true;
      any = true;
    }
  }

  if (any) check();
}

/// Whether a change at `paths` in `root` can be a change of `f`: its file, or for a page what it
/// loaded. A page's own folder is watched as a root too, whose paths start there.
function concerns(f: Follow, h: Held, root: string, paths: string[]) {
  if (!paths.length) return true;
  const dir = splitPath(f.path).dir;
  const here = root === f.root || !dir ? paths : paths.map(path => `${dir}/${path}`);
  return [f.path, ...(f.fallback === "page" ? h.loaded : [])].some(file => touches(here, file));
}

/// The stamps of `follows` as their files are now, by key: one hash ask per
/// folder for the files and those their pages loaded, else the fallback. A
/// file that could not be read has none.
async function stampAll(follows: Follow[]) {
  const folders = new Map<string, [Follow, ...Follow[]]>();

  for (const f of follows) {
    const key = `${f.machine}\n${f.root}`;
    const group = folders.get(key);
    if (group) group.push(f); else folders.set(key, [f]);
  }

  const stamps = new Map<string, Stamp | null>(), listings = new Map<string, Promise<Tree | null>>();

  await Promise.all([...folders.values()].map(async group => {
    const loaded = await Promise.all(group.map(f => f.src ? invoke<string[]>("agent_page_files", { src: f.src }).catch(() => []) : []));
    const { machine, root } = group[0];
    const hashes = await hashesOf(machine, root, [...new Set(group.flatMap((f, i) => [f.path, ...loaded[i]]))]);

    await Promise.all(group.map(async (f, i) => {
      const h = held.get(f.key);
      if (h) h.loaded = loaded[i];
      stamps.set(f.key, hashes ? hashedStamp(f.path, loaded[i], hashes) : await fallbackStamp(f, listings));
    }));
  }));

  return stamps;
}

/// The hashes of `paths` in `root` by path; null from a node from before
/// hashing, or when the machine could not be asked.
async function hashesOf(machine: string, root: string, paths: string[]) {
  const parts = Array.from({ length: Math.ceil(paths.length / HASH_PATHS) }, (_, i) => paths.slice(i * HASH_PATHS, (i + 1) * HASH_PATHS));
  const got = await Promise.all(parts.map(part => invoke<(string | null)[] | null>("agent_file_hashes", { machineId: machine, root, paths: part }).catch(() => null)));
  if (got.some((hashes, i) => !hashes || hashes.length !== parts[i].length)) return null;

  return new Map(got.flatMap((hashes, i) => parts[i].flatMap((path, j) => {
    const hash = hashes?.[j];
    return hash ? [[path, hash] as const] : [];
  })));
}

/// The stamp a node from before hashing gives: the file's listed size, its
/// text, and for a page the files listed beside and under it.
async function fallbackStamp(f: Follow, listings: Map<string, Promise<Tree | null>>): Promise<Stamp | null> {
  const { dir, name } = splitPath(f.path);
  const stamp = (value: string) => new Map([["", value]]);

  if (f.fallback === "size") {
    const tree = await listed(f.machine, f.root, dir, listings);
    const size = tree && listedSize(tree, dir, name);
    return size === null ? null : stamp(String(size));
  }

  const [text, tree] = await Promise.all([
    invoke<TreeText>("agent_tree_text", { machineId: f.machine, root: f.root, path: f.path }).catch(() => null),
    f.fallback === "page" ? listed(f.machine, f.root, dir, listings) : null,
  ]);
  if (!text || (f.fallback === "page" && !tree)) return null;

  return stamp(tree ? `${textStamp(text)}\n\n${folderStamp(tree)}` : textStamp(text));
}

/// The listing of folder `dir` in `root`, read once per check. Ignored files
/// are in it, as pages are often built into ignored folders.
function listed(machine: string, root: string, dir: string, listings: Map<string, Promise<Tree | null>>) {
  const key = JSON.stringify([machine, root, dir]);
  const had = listings.get(key);
  if (had) return had;
  const read = invoke<unknown>("agent_tree", { machineId: machine, root, path: dir, ignored: true, deep: false })
    .then(tree => isTree(tree) ? tree : null, () => null);
  listings.set(key, read);
  return read;
}
