// The first launch of an app with no subscription: start here, or link another desktop.
import { createElement } from "react";
import { flushSync } from "react-dom";
import { createDocumentView } from "../ui/document-root.tsx";
import { joinTicket } from "../machines/jointicket.ts";
import { invoke } from "../ui/tauri.ts";
import { firstRunDue } from "../settings/subscription.ts";
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

/// Shows the first-run window over the app when it is due; `after` runs once it goes, or at once
/// when it is not due. The choice is kept, so it shows once. A Mac already linked to other
/// machines has made it.
export async function firstRun(subscribed: boolean, after: () => void, signedOut = false) {
  if (!firstRunDue(subscribed, stored(), signedOut)) return after();
  if (await linked()) { keep(); return after(); }
  const host = document.createElement("div");
  document.body.append(host);
  const root = createDocumentView(host);
  let closed = false;
  const leave = () => {
    if (closed) return;
    closed = true;
    keep(); root.unmount(); host.remove(); after();
  };
  flushSync(() => root.render(createElement(FirstRunScreen, { done: leave, join: joinTicket })));
}
