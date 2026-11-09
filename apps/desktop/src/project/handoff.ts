import { invoke } from "../ui/tauri.ts";
import { el, button } from "../ui/dom.ts";
import { openSheet } from "../ui/parts.ts";
import { reason } from "../ui/format.ts";
import type { Project } from "./projects-state.ts";

export function openHandoff(project: Project, to: string, changed: () => void) {
  let closed = false;
  const { sheet } = openSheet("Bring uncommitted work", "am-sheet ip-sheet", { onClose: () => { closed = true; } });
  const destination = project.machines.find(place => place.id === to);
  const status = el("div", "am-sub", "Choose the machine holding your edits.");
  sheet.append(el("div", "am-head", el("b", "", "Bring uncommitted work"), el("kbd", "", "esc")),
    el("div", "am-sub", `Destination: ${destination?.name ?? "this machine"}. Both checkouts must be at the same commit and the destination must be clean. Edits arrive unstaged; the source copy stays intact. Ignored files stay on their machine.`), status);
  const buttons: HTMLButtonElement[] = [];

  for (const source of project.machines.filter(place => place.id !== to)) {
    const bring = button(`Bring work from ${source.name}`, "dbtn", async () => {
      buttons.forEach(button => { button.disabled = true; });
      status.textContent = "Bringing uncommitted work...";

      try {
        await invoke("project_handoff_work", { project: project.name, from: source.id, to });
        if (!closed) status.textContent = "Work brought here. The source copy is still intact.";
        changed();
      } catch (e) {
        if (!closed) {
          status.textContent = reason(e);
          buttons.forEach(button => { button.disabled = false; });
        }
      }
    });
    buttons.push(bring);
    sheet.append(bring);
  }
}
