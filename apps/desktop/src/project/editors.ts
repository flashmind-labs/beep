// "Open in" for a project's checkout on this Mac: the editors installed
// here, read once from the app, Zed first.
import { invoke } from "../ui/tauri.ts";
import { notice } from "../chat/viewer-overlay.tsx";
import { onPhone } from "../ui/platform.ts";

/// An installed editor (EditorRow).
export interface EditorRow {
  id: string;
  name: string;
}

export const CHEV = '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"><path d="M6 9l6 6 6-6"/></svg>';

let installed: EditorRow[] | null = null;
let reading = false;
const waiting = new Set<() => void>();

/// The header's buttons for `folder`: the first editor found, the others in
/// a menu beside it; none without a folder or an editor. The first call
/// reads the editors and redraws every waiting header once they are in.
export function editorsFor(folder: string | null | undefined, redraw: () => void): EditorRow[] {
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

  return folder ? installed ?? [] : [];
}

export function openEditor(ed: EditorRow, folder: string) {
  return launch(ed, folder);
}

/// Opens `folder` in the first installed editor, as the header's Open in button does.
export async function openInEditor(folder: string) {
  try {
    const [first] = await invoke<EditorRow[]>("editors");
    if (first) await launch(first, folder);
    else notice("No supported editor is installed. Install Zed, VS Code or Cursor, then restart Beep.", { bad: true });
  } catch (err) {
    notice(`Could not find an editor: ${err}`, { bad: true });
  }
}

async function launch(editor: EditorRow, folder: string) {
  try {
    await invoke("open_in_editor", { editor: editor.id, path: folder });
  } catch (err) {
    notice(`Could not open ${editor.name}: ${err}. Try opening ${editor.name} directly.`, { bad: true });
  }
}
