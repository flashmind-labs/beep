// "Open in" for a project's checkout on this Mac: the editors installed
// here, read once from the app, Zed first.
import { dbtn as button, svgEl } from "../ui/parts.ts";
import { openMenu } from "../ui/menu.ts";
import { invoke } from "../ui/tauri.ts";
import { onPhone } from "../ui/platform.ts";

/// An installed editor (EditorRow).
export interface EditorRow {
  id: string;
  name: string;
}

const CHEV = '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"><path d="M6 9l6 6 6-6"/></svg>';

let installed: EditorRow[] | null = null;
let reading = false;
const waiting = new Set<() => void>();

/// The header's buttons for `folder`: the first editor found, the others in
/// a menu beside it; none without a folder or an editor. The first call
/// reads the editors and redraws every waiting header once they are in.
export function editorTools(folder: string | null | undefined, redraw: () => void): Node[] {
  if (onPhone()) return [];
  if (installed === null) waiting.add(redraw);

  if (installed === null && !reading) {
    reading = true;
    invoke<EditorRow[]>("editors").then(
      rows => {
        installed = rows;
        reading = false;
        const redraws = [...waiting];
        waiting.clear();
        for (const draw of redraws) draw();
      },
      e => { reading = false; waiting.clear(); console.warn("editors:", e); },
    );
  }

  if (!folder || !installed?.length) return [];
  const [first, ...rest] = installed;
  const open = (ed: EditorRow, b: HTMLElement) =>
    invoke("open_in_editor", { editor: ed.id, path: folder }).catch(e => { b.title = `Could not open ${ed.name}: ${e}`; });
  const main = button(`Open in ${first.name}`, "quiet small", () => open(first, main));
  main.title = folder;
  if (!rest.length) return [main];
  const more = button(svgEl(CHEV)!, "quiet small", e => openMenu(e, "Open in", rest.map(ed => ({ label: ed.name, on: () => open(ed, main) }))));
  more.title = "Other editors";
  more.setAttribute("aria-label", "Other editors");
  return [main, more];
}

/// Opens `folder` in the first installed editor, as the header's Open in button does.
export async function openInEditor(folder: string) {
  try {
    const [first] = await invoke<EditorRow[]>("editors");
    if (first) await invoke("open_in_editor", { editor: first.id, path: folder });
  } catch (err) {
    console.warn("open in editor:", err);
  }
}
