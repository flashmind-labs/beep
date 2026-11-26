// What live reload of the pane's file tabs and picked artifact reads: the
// folder each file is watched in, and the stamps that tell a change from a
// watch event that changed nothing. The machine hashes the files on show and
// the ones a page loaded. A node from before hashing is checked by listing:
// sizes, as listings carry no times, so a binary file rewritten at the same
// size goes unseen there, and text compared whole.
import type { Tree } from "./files-state.ts";
import type { TreeText } from "./pane-state.ts";
import { readPlace } from "./artifacts-state.ts";

/// What stands for a followed file as it is now, by path: its hash, and the
/// hashes of the files its page loaded. From a node that cannot hash, one
/// entry under "" for the file and what it is checked with.
export type Stamp = ReadonlyMap<string, string>;

/// The stamp of `file` and the files its page loaded from the hashes the
/// machine gave by path; null when `file` is not there, as while it is
/// replaced.
export function hashedStamp(file: string, loaded: readonly string[], hashes: ReadonlyMap<string, string>): Stamp | null {
  if (!hashes.has(file)) return null;
  return new Map([file, ...loaded].flatMap(path => {
    const hash = hashes.get(path);
    return hash === undefined ? [] : [[path, hash] as const];
  }));
}

/// Whether `now` differs from `before` in a file both have, and the stamp
/// to keep: a file first seen, such as one its page loaded since, joins it,
/// and one missing for now keeps its last hash.
export function nextStamp(before: Stamp, now: Stamp) {
  const changed = [...now].some(([path, hash]) => {
    const was = before.get(path);
    return was !== undefined && was !== hash;
  });
  return { changed, stamp: new Map([...before, ...now]) };
}

/// A file followed: the folder watched and read from, and its path in it.
export interface Followed {
  root: string;
  path: string;
}

/// Where the file at absolute `path` is followed: in the first of `roots`
/// holding it, else in its own folder.
export function followedAt(roots: readonly string[], path: string): Followed {
  const inside = roots.map(root => readPlace(root, path)).find(place => place.inside);
  const { root, path: rel } = inside ?? readPlace("", path);
  return { root, path: rel };
}

/// The folder of `path` in its root ("" for the root itself) and its name.
export function splitPath(path: string) {
  const cut = path.lastIndexOf("/");
  return { dir: cut < 0 ? "" : path.slice(0, cut), name: path.slice(cut + 1) };
}

/// The size of file `name` in folder `dir` of a listing, or null when the
/// listing does not have it.
export function listedSize(tree: Tree, dir: string, name: string): number | null {
  const entry = tree.folders.find(f => f.path === dir)?.entries.find(e => e.name === name && !e.folder);
  return entry ? entry.size : null;
}

/// The files of a listing by name and size: a page's folder changes when one
/// beside or under it does. Git's marks are left out, as staging changes no file.
export function folderStamp(tree: Tree) {
  return tree.folders.map(f => `${f.path}\n${f.entries.map(e => `${e.name}\t${e.size}`).join("\n")}`).join("\n\n");
}

/// A text file's stamp: its size and the text read of it.
export function textStamp(text: TreeText) {
  return `${text.size}\n${text.text}`;
}
