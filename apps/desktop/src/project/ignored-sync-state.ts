export interface IgnoredSync {
  enabled: boolean;
  folders: string[];
}

export function ignoredSelection(enabled: boolean, text: string): IgnoredSync {
  const folders = [...new Set(text.split(/\r?\n/).map(p => p.trim().replace(/\/+$/, "")).filter(Boolean))].sort();
  if (folders.length > 16) throw new Error("Choose at most 16 ignored folders.");
  for (const path of folders) {
    if (path.length > 1024 || /[\\:\0\r\n]/.test(path) || path.split("/").some(p => !p || /[ .]$/.test(p) || [".", "..", ".git", ".worktrees", "target", "node_modules"].includes(p.toLowerCase()))) {
      throw new Error("Use plain project-relative folders outside Git metadata, worktrees and build output.");
    }
  }
  for (let i = 0; i < folders.length; i++) for (const other of folders.slice(i + 1)) {
    const a = folders[i].toLowerCase(), b = other.toLowerCase();
    if (a === b || a.startsWith(b + "/") || b.startsWith(a + "/")) throw new Error("Choose folders that do not overlap.");
  }
  if (enabled && !folders.length) throw new Error("Choose at least one ignored folder before enabling sync.");
  return { enabled, folders };
}
