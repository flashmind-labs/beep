// The first launch: start here, or link another desktop.
import { createElement } from "react";
import { createDocumentScope } from "../ui/document-root.tsx";
import { joinTicket } from "../machines/jointicket.ts";
import { invoke } from "../ui/tauri.ts";
import { FirstRunScreen } from "./firstrun-view.tsx";

const KEY = "beep.firstRunDone";

function stored() {
  try { return localStorage.getItem(KEY); } catch { return null; }
}

function keep() {
  try { localStorage.setItem(KEY, "1"); } catch { /* shown again next launch */ }
}

/// Whether this Mac's mesh holds other machines. The list is empty until the mesh opens, so an
/// empty one is asked again, for three seconds at most.
async function linked() {
  for (let tries = 0; tries < 30; tries++) {
    const machines = await invoke<unknown[]>("list_machines").catch(e => { console.warn("listing machines:", e); return []; });
    if (machines.length) return machines.length > 1;
    await new Promise(done => setTimeout(done, 100));
  }

  return false;
}

/// Shows the first-run window over the app until it is answered. The choice is kept, so it shows
/// once. A Mac already linked to other machines has made it: the window shows only once the mesh
/// says this Mac is alone, so it never flashes up there. Home does not wait for it.
export async function firstRun() {
  if (stored() !== null) return;
  if (await linked()) return keep();
  const screen = createDocumentScope<"first-run">();
  let closed = false;
  const leave = () => {
    if (closed) return;
    closed = true;
    keep(); screen.dispose();
  };
  screen.set([{ slot: "first-run", host: document.body, content: createElement("div", null, createElement(FirstRunScreen, { done: leave, join: joinTicket })) }]);
}
