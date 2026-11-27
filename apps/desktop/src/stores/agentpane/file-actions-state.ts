// Selection and clipboard rules for the Files tree.
export interface FileSelection {
  picked: string | null;
  anchor: string | null;
  selected: Set<string>;
}

export function selectFile(state: FileSelection, visible: readonly string[], path: string, toggle = false, range = false) {
  const selected = toggle ? new Set(state.selected) : new Set<string>();
  const start = state.anchor ? visible.indexOf(state.anchor) : -1;
  const end = visible.indexOf(path);

  if (range && start >= 0 && end >= 0) {
    for (const p of visible.slice(Math.min(start, end), Math.max(start, end) + 1)) selected.add(p);
  } else if (toggle) {
    if (selected.has(path)) selected.delete(path); else selected.add(path);
  } else selected.add(path);

  return { picked: path, anchor: range && start >= 0 ? state.anchor : path, selected };
}

export function selectionRoots(paths: readonly string[]) {
  const all = new Set(paths);
  return [...all].filter(path => {
    let at = path.lastIndexOf("/");

    while (at >= 0) {
      if (all.has(path.slice(0, at))) return false;
      at = path.lastIndexOf("/", at - 1);
    }

    return true;
  });
}

export function fileNameError(name: string) {
  return !name.trim() || name === "." || name === ".." || /[/\\:\x00-\x1f]/.test(name) || name.toLowerCase() === ".git"
    ? "Use a file name without slashes, colons, or Git metadata." : "";
}

export interface FileClipboard { key: string; paths: string[]; cut: boolean }
export function canPasteFiles(clipboard: FileClipboard | null, key: string, folder: string) {
  return !!clipboard && clipboard.key === key && clipboard.paths.length > 0
    && !clipboard.paths.some(path => path === folder || folder.startsWith(`${path}/`));
}
