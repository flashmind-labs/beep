// File and session references in the log open their context menus here.
import { localFileItems, type FileTarget } from "./localfile.ts";
import { type FileRef, linkedPath, resolvePath } from "../../stores/agentpane/media-state.ts";
import { linkedSession, parseSessionRef, type SessionLinkRef } from "../../stores/agents/nesting-state.ts";
import { openMenu, type MenuItem } from "../../ui/menu.tsx";
import { copy } from "../../ui/clipboard.ts";
import { onPhone } from "../../ui/platform.ts";
import { invoke } from "../../ui/tauri.ts";

export function fileLinkMenu(
  e: MouseEvent,
  folder: Omit<FileTarget, "path">,
  session: string,
  open: (ref: FileRef) => void,
  openSessionRef?: (ref: SessionLinkRef, beside: boolean) => void,
) {
  const ref = (e.target as Element).closest<HTMLElement>("a[href], [data-media], [data-made], a[data-path], code[data-file], code[data-session]");
  if (!ref) return;

  const sessionRef = ref.dataset.session
    ? parseSessionRef(ref.dataset.session)
    : linkedSession(ref.getAttribute("href") ?? "");
  if (sessionRef && openSessionRef) {
    const href = sessionRef.machine
      ? `beep://session/${encodeURI(sessionRef.id)}@${encodeURI(sessionRef.machine)}`
      : `beep://session/${encodeURI(sessionRef.id)}`;
    const items: MenuItem[] = [
      { label: "Open session", icon: "move", on: () => openSessionRef(sessionRef, false) },
      ...(!onPhone() ? [{ label: "Open beside", icon: "splitPane" as const, on: () => openSessionRef(sessionRef, true) }] : []),
      { label: "Copy link", icon: "copy", on: () => void copy(href) },
      { label: "Copy id", icon: "copy", on: () => void copy(sessionRef.id) },
    ];
    openMenu(e, sessionRef.id, items);
    return;
  }

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
