// The first launch: start here, or link another desktop. Start sets this computer up for
// agents when it lacks something.
import { createDocumentScope } from "../../ui/document-root.tsx";
import { joinTicket } from "../machines/jointicket.tsx";
import { buildCapabilities } from "../../ui/tauri.ts";
import { ensureMachines, MACHINES, nextMachines } from "../../stores/machines/machine-store.ts";
import { FirstRunScreen } from "./firstrun-view.tsx";
import { setupRun, thisComputer } from "./firstrun-setup.ts";

const KEY = "beep.firstRunDone";

function stored() {
  try { return localStorage.getItem(KEY); } catch { return null; }
}

function keep() {
  try { localStorage.setItem(KEY, "1"); } catch { /* shown again next launch */ }
}

/// Whether this Mac's mesh holds other machines. The list is empty until the mesh opens, so an
/// empty one waits for the next `machines` push, for three seconds at most.
async function linked() {
  let machines = await ensureMachines().catch(e => { console.warn("listing machines:", e); return []; });

  if (!machines.length) {
    await nextMachines(3000);
    machines = MACHINES.list ?? [];
  }

  return machines.length > 1;
}

/// Shows the first-run window over the app until it is answered. The choice is kept, so it shows
/// once. A Mac already linked to other machines has made it: the window shows only once the mesh
/// says this Mac is alone, so it never flashes up there. Home does not wait for it.
export async function firstRun() {
  if (stored() !== null) return;
  if (await linked()) return keep();
  if (!(await buildCapabilities()).local_host) return;
  const screen = createDocumentScope<"first-run">();
  let closed = false;
  const leave = () => {
    if (closed) return;
    closed = true;
    keep(); screen.dispose();
  };
  screen.set([{ slot: "first-run", host: document.body, content: <div><FirstRunScreen done={leave} join={joinTicket} prepare={prepare} /></div> }]);
}

/// Set up for this computer, or null when there is none to run here.
async function prepare() {
  const machine = await thisComputer();
  return machine ? setupRun(machine) : null;
}
