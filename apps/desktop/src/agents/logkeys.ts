// ⌘U and ⌘Y in a session: open or fold every thought, every command, or
// (⇧⌘Y) the last command. They write the same state a click does, then
// draw the log again with the row in view held where it is.
import { parseTool } from "./agentlog.ts";
import { splitTurns, stepOpen } from "./agentturns.ts";
import { allOpen, commandRows, thoughtRows, turnsHolding } from "./logkeys-state.ts";
import { AG } from "./agents.ts";
import { drawAgentBody, newestStep, rowInView, turnsOf } from "./logview.ts";

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
  if (!p || AG.child != null) return false;
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
  if (!p || AG.child != null) return false;
  const at = commandRows(p.entries, p.earlier).at(-1);
  if (at == null) return false;
  const keep = rowInView();
  AG.steps.set(at, !commandOpen(at));
  drawAgentBody(keep);
  return true;
}

/// ⌘U: every thought opens, or folds when all are open. Opening also opens
/// the turns that hold them, as their steps are folded into the heading.
export function toggleThoughts() {
  const p = AG.page, o = AG.open;
  if (!p || !o || AG.child != null) return false;
  const ats = thoughtRows(p.entries, p.earlier);
  if (!ats.length) return false;
  const m = turnsOf(o.session);
  const open = allOpen(ats.map(at => m.thoughts.has(at)));
  const keep = rowInView();
  for (const at of ats) {
    // The reader's choice stands when a thought ends, as a click's does.
    m.touched.add(at);
    if (open) m.thoughts.add(at);
    else m.thoughts.delete(at);
  }
  if (open) for (const key of turnsHolding(splitTurns(p.entries, p.earlier), ats)) m.opened.add(key);
  drawAgentBody(keep);
  return true;
}
