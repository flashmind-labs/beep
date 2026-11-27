// The command center's Terminals page, and the terminals Home lists.
import { Button } from "../ui/kit.tsx";
import { ALL, byMachine, terminalsCount } from "./terminal-list.ts";
import type { CenterView } from "../center/state.ts";
import { drawPage } from "../center/nav.ts";
import { allTerms, ensureAllTerms, termMachines } from "../center/allterms.ts";
import { machinesPanel } from "./terminals.tsx";
import { ensureHome } from "../center/refresh.ts";

/// The Terminals page into its view, keeping the list's scroll.
function drawTerminals() {
  drawPage("terminals");
}

export function TerminalsPage() {
  return <div className="cc-view tl-view">{machinesPanel(allTerms, termMachines())}</div>;
}

export const terminalsView: CenterView<"terminals"> = {
  title() {
    const list = allTerms.list(ALL);
    const sub = list ? terminalsCount(byMachine(termMachines(), list)) : "";
    const add = <Button look="primary" small disabled={!termMachines().some(m => m.online)} onClick={() => allTerms.newOn(termMachines().find(m => m.online)?.id || null)}>New terminal</Button>;
    return { title: "Terminals", sub, reactTools: list === null ? null : add };
  },
  mount() {
    // The list the window holds draws at once; the terminals topic keeps it.
    drawTerminals();
    ensureAllTerms();
    ensureHome();
  },
  unmount() {
    // Detached, not stopped: they keep running on their machines.
    allTerms.stop();
    drawPage("terminals", false);
  },
};
