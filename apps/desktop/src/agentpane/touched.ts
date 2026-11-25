// What an update of a folder's `files` topic says changed in it. The node names the paths
// under the folder (`.git` for Git's metadata outside it), at most 256; an empty list means it
// does not say, as from a node from before it, and anything may have changed.

const record = (value: unknown): value is Record<string, unknown> => typeof value === "object" && value !== null;

/// The changed paths an update carries, in the views native code sends or as the node says them.
export function changedIn(data: unknown): string[] {
  const list = !record(data) ? null
    : Array.isArray(data.changed) ? data.changed
    : record(data.Sessions) && record(data.Sessions.Tree) ? data.Sessions.Tree.changed : null;

  return Array.isArray(list) ? list.filter((path): path is string => typeof path === "string") : [];
}

const GIT_READ = ["HEAD", "index", "packed-refs", "refs"];

/// Whether a change can alter what the folder's checkout shows: its files, or the Git state the
/// diff and history read (HEAD, the index and refs).
export function touchesRepo(paths: readonly string[]) {
  return !paths.length || paths.some(path => {
    if (path === ".git") return true;
    if (!path.startsWith(".git/")) return true;
    return GIT_READ.some(name => path === `.git/${name}` || path.startsWith(`.git/${name}/`));
  });
}

/// Whether a change touches `file`, or a folder holding it or a file under it.
export function touches(paths: readonly string[], file: string) {
  return !paths.length || paths.some(path => path === file || file.startsWith(`${path}/`) || path.startsWith(`${file}/`));
}
