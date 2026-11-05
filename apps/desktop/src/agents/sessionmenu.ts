// The right-click menu of a coding agent session, shared by every row that shows one.
import { confirmSheet, openMenu, type MenuItem } from "../ui/menu.ts";
import { writeClipboard } from "../ui/parts.ts";
import { invoke } from "../ui/tauri.ts";
import { openInEditor } from "../project/editors.ts";
import type { AgentRow, Started } from "./agents.ts";
import { sessionOffers } from "./sessionmenu-state.ts";

/// Opens the menu for `s` where `e` was clicked. `open` is what clicking the row does.
export async function sessionMenu(e: MouseEvent, s: AgentRow, open: (s: AgentRow) => void) {
  e.preventDefault();
  e.stopPropagation();
  const offers = sessionOffers(s);
  const at = { machineId: s.machine_id, agent: s.agent, session: s.id };
  // Only a folder this Mac has can be opened here.
  const folder = await invoke<string | null>("session_folder_here", { machineId: s.machine_id, cwd: s.cwd }).catch(() => null);
  const items: MenuItem[] = [{ label: "Open", on: () => open(s) }];
  if (offers.stop) items.push({ label: "Stop turn", on: () => void invoke("agent_steer", { ...at, steer: "cancel" }).catch(err => console.warn("stop turn:", err)) });
  if (offers.resume) {
    items.push({ label: "Resume", on: () => void invoke<Started>("agent_resume", at).then(started => open(started.session), err => console.warn("resume:", err)) });
  }
  if (folder) {
    items.push({ label: "Open in editor", on: () => void openInEditor(folder) });
    items.push({ label: "Open folder", on: () => void invoke("open_folder", { path: folder }).catch(err => console.warn("open folder:", err)) });
  }
  items.push({ label: "Copy id", on: () => void writeClipboard(s.id) });
  if (offers.close) {
    items.push({
      label: "Close session…",
      warn: true,
      on: () => confirmSheet({
        title: "Close this session?",
        text: "The agent stops running.",
        note: "Its transcript stays.",
        action: "Close",
        run: () => invoke("agent_steer", { ...at, steer: "close" }),
      }),
    });
  }
  openMenu(e, s.title || "Session", items);
}
