// The first launch of an app with no subscription: start here, or link another desktop.
import { joinFields } from "../machines/jointicket.ts";
import { button, el } from "../ui/dom.ts";
import { svgEl } from "../ui/parts.ts";
import { invoke } from "../ui/tauri.ts";
import { firstRunDue } from "../settings/subscription.ts";

const KEY = "beep.firstRunDone";

function stored() {
  try {
    return localStorage.getItem(KEY);
  } catch {
    return null;
  }
}

function keep() {
  try {
    localStorage.setItem(KEY, "1");
  } catch { /* shown again next launch */ }
}

/// flashmind's mark.
const LOGO = '<svg width="34" height="34" viewBox="0 0 28 28" fill="currentColor" aria-hidden="true"><path d="M14 1 27 27h-3.1L15.4 10v17h-2.8V10L4.1 27H1Z"/></svg>';

const big = (title: string, text: string, cls: string, onClick: () => void) =>
  button(el("span", "fr-big", el("b", "", title), el("span", "", text)), `dbtn fr-btn ${cls}`, onClick);

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
  const root = el("div", "fr-scrim");
  const leave = () => { keep(); root.remove(); after(); };
  const card = el("div", "fr-card");
  const choose = () => {
    card.replaceChildren(
      el("div", "fr-head", svgEl(LOGO), el("div", "", el("b", "", "Welcome to Beep"), el("div", "fr-sub", "Run coding agents on your machines."))),
      big("Start", "Use Beep on this Mac. You can add machines later.", "pri", leave),
      big("Link other desktop", "You already use Beep on another Mac. Share its machines, projects and keys with this one.", "", link));
    card.querySelector("button")?.focus();
  };
  const link = () => {
    const fields = joinFields(leave, button("Back", "dbtn quiet", choose));
    card.replaceChildren(
      el("div", "fr-head", svgEl(LOGO), el("div", "", el("b", "", "Link other desktop"),
        el("div", "fr-sub", "On your other desktop, open Machines, then Add a machine, and copy the ticket. This Mac then shares its machines, projects and keys."))),
      ...fields.nodes);
    fields.input.focus();
  };
  card.setAttribute("role", "dialog");
  card.setAttribute("aria-label", "First run");
  root.append(card);
  document.body.append(root);
  choose();
}
