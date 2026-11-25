// File references in the log use the same local apps as the Files tab.
import { localFileItems, type FileTarget } from "../agentpane/localfile.ts";
import { type FileRef, linkedPath, resolvePath } from "../agentpane/media-state.ts";
import { openMenu, type MenuItem } from "../ui/menu.tsx";
import { copy } from "../ui/clipboard.ts";
import { invoke } from "../ui/tauri.ts";

export function fileLinkMenu(e: MouseEvent, folder: Omit<FileTarget, "path">, session: string, open: (ref: FileRef) => void) {
  const ref = (e.target as Element).closest<HTMLElement>("a[href], [data-media], [data-made], a[data-path], code[data-file]");
  if (!ref) return;
  const plain = ref.dataset.media ?? ref.dataset.made ?? ref.dataset.path;
  const file = plain ? { path: plain, line: null } : linkedPath(ref.dataset.file ?? ref.getAttribute("href") ?? "");
  if (!file) return;
  const path = file.path;
  const items: MenuItem[] = [
    { label: "Open in Beep", on: () => open(file) },
    { label: "Copy path", on: () => void copy(path) },
  ];
  const root = resolvePath(folder.root, ".").replace(/\/+$/, "");
  const full = resolvePath(root, path);
  const fallback: MenuItem = { label: "Open in default app", on: () => {
    void invoke("agent_file_open", { machineId: folder.machineId, path: full, session })
      .catch(error => window.alert(`Could not open file: ${error}`));
  } };
  const update = openMenu(e, path, [...items, fallback]);
  // Local actions require a file inside the session folder.
  if (!full.startsWith(`${root}/`)) return;
  void localFileItems({ ...folder, path: full.slice(root.length + 1) }, { x: e.clientX, y: e.clientY }).then(more => {
    if (!more.length) return;
    update([...items, ...more.map(item => item.label === "Open" ? { ...item, label: "Open in default app" } : item)]);
  });
}
