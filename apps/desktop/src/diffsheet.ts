// A branch's diff, over the window: View diff on the Branches list, a
// review and a task with a branch. The node with the project's checkout
// reads it against the default branch, the branch's worktree with its
// uncommitted changes when it has one. Esc or Close goes back.
import type { DiffView } from "./diff-state.ts";
import { diffView } from "./diffview.ts";
import { invoke } from "./tauri.ts";

let open: HTMLElement | null = null;

export function openBranchDiff(project: string, branch: string) {
  open?.remove();
  const back = document.activeElement as HTMLElement | null;
  const sheet = document.createElement("div");
  sheet.className = "dv-sheet";
  sheet.setAttribute("role", "dialog");
  sheet.setAttribute("aria-modal", "true");
  sheet.setAttribute("aria-label", `Changes on ${branch}`);
  const box = document.createElement("div");
  box.className = "box";
  const top = document.createElement("div");
  top.className = "top";
  const title = document.createElement("h3");
  title.textContent = branch;
  const sub = document.createElement("small");
  sub.textContent = `${project}, against its default branch`;
  const gap = document.createElement("span");
  gap.style.flex = "1";
  const close = document.createElement("button");
  close.className = "dv-btn";
  close.type = "button";
  close.textContent = "Close";
  close.title = "Close (esc)";
  top.append(title, sub, gap, close);
  const view = diffView({
    load: (only, lockfiles) => invoke<DiffView>("branch_diff", { project, branch, only, lockfiles }),
  });
  box.append(top, view.el);
  sheet.append(box);
  const done = () => {
    sheet.remove();
    if (open === sheet) open = null;
    back?.focus?.({ preventScroll: true });
  };
  close.addEventListener("click", done);
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
