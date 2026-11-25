// ⌘U and ⌘Y in a session: open or fold every thought, every command, or
// (⇧⌘Y) the last command. They write the same state a click does, then
// draw the log again with the row in view held where it is.
import { parseTool } from "./agentlog.ts";
import { splitTurns, stepOpen, turnSections } from "./agentturns.ts";
import { allOpen, commandRows, thoughtRows } from "./logkeys-state.ts";
import { AG } from "./agents-state.ts";
import { drawAgentBody, newestStep, rowInView, turnsOf } from "./logview.tsx";

/// Whether the command at `at` shows open now.
function commandOpen(at: number) {
  const p = AG.page!;
  const failed = parseTool(p.entries[at - p.earlier].text).status === "failed";
  return stepOpen(failed, at === newestStep, AG.steps.get(at));
}

/// ⌘Y: every command row opens, or folds when all are open. An opened row
/// leaves its "Ran N commands" group and shows under a folded turn's heading.
export function toggleCommands() {
  const p = AG.page;
  if (!p) return false;
  const ats = commandRows(p.entries, p.earlier);
  if (!ats.length) return false;
  const open = allOpen(ats.map(commandOpen));
  const keep = rowInView();
  for (const at of ats) AG.steps.set(at, open);
  drawAgentBody(keep);
  return true;
}

/// ⇧⌘Y: the last command row opens or folds.
export function toggleLastCommand() {
  const p = AG.page;
  if (!p) return false;
  const at = commandRows(p.entries, p.earlier).at(-1);
  if (at == null) return false;
  const keep = rowInView();
  AG.steps.set(at, !commandOpen(at));
  drawAgentBody(keep);
  return true;
}

/// ⌘U: every thought opens, or folds when all are open.
export function toggleThoughts() {
  const p = AG.page, o = AG.open;
  if (!p || !o) return false;
  const ats = thoughtRows(p.entries, p.earlier);
  if (!ats.length) return false;
  const m = turnsOf(o.session);
  const open = allOpen(ats.map(at => m.thoughts.has(at)));
  const keep = rowInView();

  if (open) {
    const wanted = new Set(ats);

    for (const turn of splitTurns(p.entries, p.earlier)) {
      for (const section of turnSections(p.entries, turn, p.earlier)) {
        if (section.progress && section.ats.some(at => wanted.has(at))) m.opened.add(section.ats[0]);
      }
    }
  }

  for (const at of ats) {
    if (open) m.thoughts.add(at);
    else m.thoughts.delete(at);
  }

  drawAgentBody(keep);
  return true;
}
