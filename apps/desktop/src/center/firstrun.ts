// The first launch of an app with no subscription: Join with a ticket, or use this Mac alone.
import { joinFields } from "../machines/jointicket.ts";
import { button, el } from "../ui/dom.ts";
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

/// Shows the first-run window over the app when it is due; `after` runs once it goes, or at once
/// when it is not due. The choice is kept, so it shows once.
export function firstRun(subscribed: boolean, after: () => void, signedOut = false) {
  if (!firstRunDue(subscribed, stored(), signedOut)) return after();
  const root = el("div", "fr-scrim");
  const leave = () => { keep(); root.remove(); after(); };
  const alone = button("Use this Mac alone", "dbtn quiet", leave);
  const fields = joinFields(leave, alone);
  const card = el("div", "fr-card",
    el("div", "", el("b", "", "Beep"), el("div", "fr-sub", "First run on this Mac")),
    el("label", "fr-label", "Join with a ticket"),
    ...fields.nodes);
  card.setAttribute("role", "dialog");
  card.setAttribute("aria-label", "First run");
  root.append(card);
  document.body.append(root);
  fields.input.focus();
}
