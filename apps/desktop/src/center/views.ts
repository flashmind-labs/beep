import { refreshTopicSubscriptions } from "../ui/mesh-events.ts";
// The window's views: registering one, going to one, and the header it draws.
import { invoke, recordTiming } from "../ui/tauri.ts";
import type { PaneTarget } from "../bar/pane.ts";
import { el as h } from "../ui/dom.ts";
import { mainTarget, route, type CenterTarget, type ProjectScope } from "../project/projectwindow.ts";
import { chatProject } from "../project/agent.ts";
import { type ProjectsArg, type ViewArg, type ViewHead, type CenterView, type ViewName, $, state, views } from "./state.ts";
import { drawSide, projectRows } from "./sidebar.ts";
import { openSettings } from "../settings/sheet.ts";
import { threadChats } from "./threadcard.ts";
import { PageHistory } from "./pagehistory.ts";
import { PageTiming, pageName } from "./page-timing.ts";
import { viewHidden } from "../settings/subscription.ts";
import { S } from "../bar/core.ts";

export let shown: ViewName | null = null;
/// The argument `shown` was opened with, so Esc from an agent view can return to it.
export let shownArg: ViewArg;

/// Adds a view the window can show.
export function registerView(name: ViewName, view: CenterView) {
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

/// Opens research `session` as a page of the window.
export function openResearch(session: string) {
  return go("researchopen", { session });
}

/// Shows `target` in the main window, bringing it up.
export function toMain(target: CenterTarget) {
  invoke("center_open", { target }).catch(e => console.warn("opening in the main window:", e));
}

const pages = new PageHistory<{ view: ViewName; arg: ViewArg }>();
/// Set while ⌘[ or ⌘] moves, so the move is not a new visit.
let moving = false;
const pageTiming = new PageTiming(() => performance.now(), callback => { requestAnimationFrame(callback); }, () => !document.hidden, (page, ms) => recordTiming("page", page, ms));

/// The page showing, as `go` opens it again.
function here() {
  return { view: shown!, arg: views.get(shown!)?.currentArg?.() ?? shownArg };
}

/// Goes one page back (⌘[) or forward (⌘]), if there is one.
export function goHistory(forward: boolean) {
  if (!shown) return;
  const page = forward ? pages.forward(here()) : pages.back(here());
  if (!page) return;
  moving = true;
  try {
    go(page.view, page.arg);
  } finally {
    moving = false;
  }
}

/// Shows view `name`, passing `arg` to its mount.
export function go(name: ViewName, arg?: ViewArg): boolean {
  // A link to a page only a subscription has lands on Home, and a new or old chat on the
  // agent's start form.
  if (viewHidden(name, arg, state.subscribed, S.server)) {
    const chat = !!arg && typeof arg === "object" && "chat" in arg;
    return chat ? go("pane", { agentnew: { prompt: "" } }) : go("home");
  }
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
  // Keep the return page when a notification replaces an open conversation.
  if (name === "pane" && opensAgent(arg) && !arg.from && shown) {
    const current = here();
    if (shown !== "pane") arg = { ...arg, from: current };
    else if (current.arg && typeof current.arg === "object" && "draft" in current.arg) {
      arg = { ...arg, from: { view: "projects", arg: { project: current.arg.draft.project } } };
    } else if (opensAgent(current.arg) && current.arg.from) {
      arg = { ...arg, from: current.arg.from };
    }
  }
  const view = views.get(name);
  if (!view) return false;
  const afterMount = pageTiming.start(pageName(name, arg));
  // Sending from a draft turns it into the session; Back skips the spent form.
  const replaceDraft = shown === "pane" && !!shownArg && typeof shownArg === "object" && "draft" in shownArg
    && name === "pane" && !!arg && typeof arg === "object" && "agent" in arg && arg.adopt === true;
  if (shown && !moving) pages.visit(here(), { view: name, arg: name === "projects" ? { project: (arg as ProjectsArg | undefined)?.project ?? null, tile: (arg as ProjectsArg | undefined)?.tile ?? null } : arg }, replaceDraft);
  shownArg = arg;
  if (shown && shown !== name) views.get(shown)!.unmount?.();
  shown = name;
  for (const [n, v] of views) v.el.hidden = n !== name;
  const mounted = view.mount(view.el, arg);
  refreshTopicSubscriptions();
  drawTop();
  drawSide();
  if (mounted) void mounted.then(() => afterMount(name === "pane" ? pageName(name, S.view) : undefined)).catch(e => console.warn("mounting page:", e));
  else afterMount();
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
