// Open, Open With and Reveal in Finder for a file of a session's folder that
// this Mac has. The menus of the Files tab and the Changes tab share these
// lines; the folder, the file and the app are all resolved in Rust.

import { openMenu, type MenuItem } from "../ui/menu.ts";
import { invoke } from "../ui/tauri.ts";

/// An app macOS says can open a file (OpenApp).
export interface OpenApp {
  id: string;
  name: string;
  default: boolean;
}

/// A file of a session on a machine: `path` is under `root`.
export interface FileTarget {
  machineId: string;
  root: string;
  path: string;
}

type Action = "open" | "reveal" | { with: string };

function run(t: FileTarget, action: Action) {
  invoke("open_local_file", { ...t, action }).catch(e => console.warn("open file:", e));
}

/// The menu lines for `t`: none when the file is not on this Mac. `at` is
/// where Open With opens its list of apps.
export async function localFileItems(t: FileTarget, at: { x: number; y: number }): Promise<MenuItem[]> {
  let file: { apps: OpenApp[] } | null = null;
  try {
    file = await invoke<{ apps: OpenApp[] } | null>("local_file", { ...t });
  } catch (e) {
    console.warn("local file:", e);
  }
  if (!file) return [];
  const { apps } = file;
  const items: MenuItem[] = [{ label: "Open", on: () => run(t, "open") }];
  if (apps.length) {
    items.push({
      label: "Open With",
      on: () => {
        const e = new MouseEvent("contextmenu", { clientX: at.x, clientY: at.y });
        openMenu(e, "Open With", apps.map(a => ({ label: a.default ? `${a.name} (default)` : a.name, on: () => run(t, { with: a.id }) })));
      },
    });
  }
  items.push({ label: "Reveal in Finder", on: () => run(t, "reveal") });
  return items;
}
