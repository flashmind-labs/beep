// A branch's diff, over the window: View diff on the Branches list, a review
// and a task with a branch.
import type { DiffView } from "./diff-state.ts";
import { diffView } from "./diff.ts";
import { invoke } from "../ui/tauri.ts";
import { button, el } from "../ui/dom.ts";

let open: HTMLElement | null = null;

export function openBranchDiff(project: string, branch: string) {
  open?.remove();
  const back = document.activeElement as HTMLElement | null;
  const sheet = el("div", "dv-sheet");
  sheet.setAttribute("role", "dialog");
  sheet.setAttribute("aria-modal", "true");
  sheet.setAttribute("aria-label", `Changes on ${branch}`);
  const gap = el("span");
  gap.style.flex = "1";
  const close = button("Close", "dv-btn", () => done());
  close.title = "Close (esc)";
  const view = diffView({
    load: (only, lockfiles) => invoke<DiffView>("branch_diff", { project, branch, only, lockfiles }),
  });
  sheet.append(el("div", "box", el("div", "top", el("h3", "", branch), el("small", "", `${project}, against its default branch`), gap, close), view.el));
  const done = () => {
    sheet.remove();
    if (open === sheet) open = null;
    back?.focus?.({ preventScroll: true });
  };
  sheet.addEventListener("click", e => { if (e.target === sheet) done(); });
  sheet.addEventListener("keydown", e => {
    if (e.key !== "Escape" || e.defaultPrevented) return;
    e.preventDefault();
    e.stopPropagation();
    done();
  });
  document.body.append(sheet);
  open = sheet;
  view.focus();
}
