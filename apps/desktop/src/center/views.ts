// The window's views: registering one, going to one, and the header it draws.
import { invoke } from "../ui/tauri.ts";
import type { PaneTarget } from "../bar/pane.ts";
import { el as h } from "../ui/dom.ts";
import { mainTarget, route, type CenterTarget, type ProjectScope } from "../project/projectwindow.ts";
import { chatProject } from "../project/agent.ts";
import { type ViewArg, type ViewHead, type CenterView, $, state, views } from "./state.ts";
import { drawSide, projectRows } from "./sidebar.ts";
import { openSettings } from "../settings/sheet.ts";
import { threadChats } from "./threadcard.ts";

export let shown: string | null = null;
/// The argument `shown` was opened with, so Esc from an agent view can return to it.
export let shownArg: ViewArg;

/// Adds a view the window can show.
export function registerView(name: string, view: CenterView) {
  let el = view.el;
  if (!el) {
    el = h("div", "cc-box");
    el.hidden = true;
    $("#main").append(el);
  }
  views.set(name, { ...view, el });
}

/// A pane target that opens an agent view, which can go back.
function opensAgent(arg: ViewArg): arg is Extract<PaneTarget, { agent: unknown } | { agentnew: unknown }> {
  return !!arg && typeof arg === "object" && ("agent" in arg || "agentnew" in arg);
}

/// The project a project window shows alone; null in the main window.
export let windowProject: string | null = null;
export function setWindowProject(name: string | null) { windowProject = name; }
/// Chats this project window opened from its page, such as a PR's, kept
/// in it before the thread chats list has them.
export const ownChats = new Set<string>();

/// What a project window keeps for itself; null in the main window.
function windowScope(): ProjectScope | null {
  const project = windowProject;
  if (!project) return null;
  const p = (projectRows() || []).find(r => r.name === project);
  return {
    project,
    chatProject: s => chatProject(state.agents, s)?.name ?? threadChats?.get(s)?.project ?? (ownChats.has(s) ? project : null),
    folders: p?.machines ?? [],
  };
}

/// Shows `target` in the main window, bringing it up.
export function toMain(target: CenterTarget) {
  invoke("center_open", { target }).catch(e => console.warn("opening in the main window:", e));
}

/// Shows view `name`, passing `arg` to its mount.
export function go(name: string, arg?: ViewArg): boolean {
  if (route(windowScope(), name, arg) === "main") {
    toMain(mainTarget(name, arg));
    return false;
  }
  // Settings open over whatever shows, which stays as it is.
  if (name === "pane" && arg === "settings") {
    if (!shown) go("home");
    openSettings();
    return true;
  }
  // An agent view opened from another view goes back there on Esc.
  if (name === "pane" && opensAgent(arg) && !arg.from && shown && shown !== "pane") {
    arg = { ...arg, from: { view: shown, arg: shown === "projects" ? { project: state.picked } : shownArg } };
  }
  shownArg = arg;
  const view = views.get(name);
  if (!view) return false;
  if (shown && shown !== name) views.get(shown)!.unmount?.();
  shown = name;
  for (const [n, v] of views) v.el.hidden = n !== name;
  view.mount(view.el, arg);
  drawTop();
  drawSide();
  return true;
}

export function drawTop() {
  const view = shown ? views.get(shown) : undefined;
  const head: ViewHead = typeof view?.title === "function" ? view.title() : { title: view?.title ?? "Beep" };
  // A project window's other views go back to the project.
  const back = document.getElementById("pw-back");
  if (back) back.hidden = shown === "projects";
  $("#title").textContent = head.title || "Beep";
  $("#scope").replaceChildren(...(head.scope ? [head.scope] : []));
  $("#date").textContent = head.sub || "";
  $("#tools").replaceChildren(...(head.tools || []));
}
