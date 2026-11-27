export interface IgnoredSync {
  enabled: boolean;
  folders: string[];
}

/// What a `.gitignore` names plainly, as suggestions: no globs, negations
/// or build output. Folders can sync; files cannot yet.
export interface GitignoreSuggestions {
  folders: string[];
  files: string[];
}

export function gitignoreSuggestions(gitignore: string): GitignoreSuggestions {
  const paths = gitignore.split(/\r?\n/).map(line => line.trim())
    .filter(line => line && !/^[#!]|[*?[\\]/.test(line));
  const plain = (path: string) => path.replace(/^\/+|\/+$/g, "");
  const folders = paths.map(plain)
    .filter(path => plainFolder(path) && !path.split("/").some(part => part.includes(".") || BUILD.includes(part.toLowerCase())));
  const files = paths.filter(path => !path.endsWith("/") && /\.[^/]*$/.test(path)).map(plain)
    .filter(path => path && path !== ".DS_Store" && !path.split("/").some(part => BUILD.includes(part.toLowerCase())));

  return { folders: [...new Set(folders)], files: [...new Set(files)] };
}

/// Folder names that read as build output, which stays on each machine.
const BUILD = ["dist", "build", "out", "coverage", "venv"];

function plainFolder(path: string) {
  return path.length <= 1024 && !/[\\:\0\r\n]/.test(path) && path.split("/").every(p => p && !/[ .]$/.test(p) && ![".", "..", ".git", ".worktrees", "target", "node_modules"].includes(p.toLowerCase()));
}

/// The folders listed in `text`, one per line.
export function listedFolders(text: string): string[] {
  return text.split(/\r?\n/).map(p => p.trim().replace(/\/+$/, "")).filter(Boolean);
}

export function ignoredSelection(enabled: boolean, text: string): IgnoredSync {
  const folders = [...new Set(listedFolders(text))].sort();
  if (folders.length > 16) throw new Error("Choose at most 16 ignored folders.");

  if (!folders.every(plainFolder)) throw new Error("Use plain project-relative folders outside Git metadata, worktrees and build output.");

  for (let i = 0; i < folders.length; i++) for (const other of folders.slice(i + 1)) {
    const a = folders[i].toLowerCase(), b = other.toLowerCase();
    if (a === b || a.startsWith(b + "/") || b.startsWith(a + "/")) throw new Error("Choose folders that do not overlap.");
  }
  if (enabled && !folders.length) throw new Error("Choose at least one ignored folder before enabling sync.");
  return { enabled, folders };
}
