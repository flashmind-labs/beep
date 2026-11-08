// File references in the log use the same local apps as the Files tab.
import { localFileItems, type FileTarget } from "../agentpane/localfile.ts";
import { linkedPath, resolvePath } from "../agentpane/media-state.ts";
import { openMenu, type MenuItem } from "../ui/menu.ts";
import { copy } from "../ui/parts.ts";

export function fileLinkMenu(e: MouseEvent, folder: Omit<FileTarget, "path">, open: (path: string) => void) {
  const ref = (e.target as Element).closest<HTMLElement>("a[href], [data-media], [data-made], a[data-path]");
  if (!ref) return;
  const path = ref.dataset.media ?? ref.dataset.made ?? ref.dataset.path ?? linkedPath(ref.getAttribute("href") ?? "");
  if (!path) return;
  const items: MenuItem[] = [
    { label: "Open in Beep", on: () => open(path) },
    { label: "Copy path", on: () => void copy(path) },
  ];
  const update = openMenu(e, path, items);
  const root = resolvePath(folder.root, ".").replace(/\/+$/, "");
  const full = resolvePath(root, path);
  // The native command accepts only files inside this session's folder.
  if (!full.startsWith(`${root}/`)) return;
  void localFileItems({ ...folder, path: full.slice(root.length + 1) }, { x: e.clientX, y: e.clientY }).then(more => {
    update([...items, ...more.map(item => item.label === "Open" ? { ...item, label: "Open in default app" } : item)]);
  });
}
