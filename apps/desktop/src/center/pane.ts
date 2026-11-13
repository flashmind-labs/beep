// The bar's page as a pane in the window: its header, and handoffs from
// other windows.
import { createElement } from "react";
import { invoke, listen } from "../ui/tauri.ts";
import { bar, type View } from "../bar/core.ts";
import { pane } from "../bar/pane.ts";
import type { CenterTarget } from "../project/projectwindow.ts";
import { type CenterView, type ViewHead, state } from "./state.ts";
import { currentLocation, go } from "./views.ts";
import { drawSide, paneOn } from "./sidebar.ts";
import { ensureHome } from "./refresh.ts";
import { showNotices } from "./notices.ts";
import { openReport } from "../report/report.ts";
import { debugPrompt } from "../report/report-state.ts";
import type { MacControl } from "../machines/machine-state.ts";

/// Titles of the pane's views.
const PANE_TITLES: Record<View, [string, string]> = {
  settings: ["Settings", ""],
  machines: ["Settings", ""],
  waiting: ["Sessions", ""],
  agents: ["Coding agents", "On your machines"],
  agentnew: ["Start a coding agent", "It runs on your machine; follow it and answer it here"],
  agent: ["Coding agents", "Follow it and answer it here"],
};

function paneTitle(): ViewHead {
  const on = paneOn();
  const [title, sub] = on.view ? PANE_TITLES[on.view] : ["Beep", ""];
  // The phone hides the list's own header, so New sits in the page header there.
  if (on.view === "agents") return { title, sub, reactTools: createElement("button", { type: "button", className: "dbtn small ag-new-top", onClick: () => go("pane", { agentnew: { prompt: "" } }) }, "New") };
  if (on.view === "agentnew") return { title, sub, reactTools: createElement("button", { type: "button", className: "dbtn small", onClick: () => go("pane", "agents") }, "All sessions") };
  return { title, sub };
}

export const paneView: CenterView<"pane"> = {
  title: paneTitle,
  mount(_el, arg) {
    pane.mounted(true);
    return Promise.resolve(pane.open(arg)).then(() => {});
  },
  unmount() {
    pane.mounted(false);
    pane.leave();
  },
};

bar.dataset.view = pane.current().view;
pane.onChange(now => {
  bar.dataset.view = now.view;
  drawSide();
});

// ---------- Handoffs ----------

/// The Report a problem sheet's "Debug with an agent": the start form on this
/// Mac, when Beep may use it, with a prompt to look into Beep and file a report.
export async function debugWithAgent(text: string) {
  const mac = state.mac ?? await invoke<MacControl>("mac_control").catch(() => null);
  go("pane", { agentnew: { prompt: debugPrompt(text), at: mac?.enabled ? { machine: mac.id } : null } });
}

/// What the bar or tray asked the window to show, taken once.
export const readPending = () => invoke<CenterTarget | null>("center_pending").catch(() => null);

/// Shows what the bar or tray asked for, once; Home when nothing is waiting.
export async function takePending(fallback: "home" | null) {
  showPending(await readPending(), fallback);
}

/// Shows `target`, read with `readPending`; `fallback` when nothing was waiting.
export function showPending(target: CenterTarget | null, fallback: "home" | null) {
  if (target == null) return fallback && go(fallback);
  if (target === "home" || target === "instructions" || target === "providers" || target === "terminals") return go(target);

  // The report target: the Report a problem sheet over the page on screen.
  if (target === "report") {
    if (!currentLocation()) go("home");
    return openReport(document.body, debugWithAgent);
  }

  if (typeof target === "object" && "projects" in target) return go("projects", target.projects);

  // A notification's click: the page behind stays, the pane opens over it.
  if (typeof target === "object" && "notice" in target) {
    if (!currentLocation()) go("home");
    return showNotices();
  }

  go("pane", target);
  // Home is not on screen, but the sidebar counts come from it. After the
  // first load its topics keep them current.
  ensureHome();
}

listen("center-open", () => takePending(null));
