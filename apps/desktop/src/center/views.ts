import { refreshTopicSubscriptions } from "../ui/mesh-events.ts";
// The window's views: registering one, going to one, and the header it draws.
import { invoke, recordTiming } from "../ui/tauri.ts";
import type { PaneTarget } from "../bar/pane.ts";
import { el as h } from "../ui/dom.ts";
import { mainTarget, route, type CenterTarget, type ProjectScope } from "../project/projectwindow.ts";
import { chatProject } from "../project/agent.ts";
import { type Location, type NavigationArgs, type ViewHead, type CenterView, type ViewName, $, state, views } from "./state.ts";
import { drawSide, projectRows } from "./sidebar.ts";
import { openSettings } from "../settings/sheet.ts";
import { threadChats } from "./threadcard.ts";
import { PageHistory } from "./pagehistory.ts";
import { PageTiming, pageName } from "./page-timing.ts";
import { viewHidden } from "../settings/subscription.ts";
import { G, S } from "../bar/core.ts";
import { syncMobile } from "./mobile.ts";
import { patchChildren } from "../ui/patch.ts";

let currentPage: Location | null = null;

/// Adds a view the window can show.
export function registerView<K extends ViewName>(name: K, view: CenterView<NoInfer<K>>) {
  let el = view.el;

  if (!el) {
    el = h("div", "cc-box");
    el.hidden = true;
    $("#main").append(el);
  }

  views.set(name, { ...view, el });
}

/// A pane target that opens an agent view, which can go back.
function opensAgent(arg: PaneTarget): arg is Extract<PaneTarget, { agent: unknown } | { agentnew: unknown }> {
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

const pages = new PageHistory<Location>();
/// Set while ⌘[ or ⌘] moves, so the move is not a new visit.
let moving = false;
const pageTiming = new PageTiming(() => performance.now(), callback => { requestAnimationFrame(callback); }, () => !document.hidden, (page, ms) => recordTiming("page", page, ms));

/// The page showing, as `go` opens it again.
export function currentLocation(): Location | null {
  if (!currentPage) return null;

  // Project sections change without mounting the page again.
  if (currentPage.view === "projects") {
    const arg = views.get("projects")?.currentArg?.();
    if (arg && typeof arg === "object" && "project" in arg) return { view: "projects", arg };
  }

  return currentPage;
}

/// Goes one page back (⌘[) or forward (⌘]), if there is one; false when there is none.
export function goHistory(forward: boolean): boolean {
  const current = currentLocation();
  if (!current) return false;
  const page = forward ? pages.forward(current) : pages.back(current);
  if (!page) return false;
  moving = true;

  try {
    goTo(page);
  } finally {
    moving = false;
  }

  return true;
}

/// Shows view `name`, passing `arg` to its mount.
export function go(...[name, arg]: NavigationArgs): boolean {
  switch (name) {
    case "home": return goTo({ view: name, arg: null });
    case "research": return goTo({ view: name, arg: null });
    case "sync": return goTo({ view: name, arg: null });
    case "terminals": return goTo({ view: name, arg: null });
    case "pane": return goTo({ view: name, arg: arg ?? null });
    case "projects": return goTo({ view: name, arg: arg ?? null });
    case "providers": return goTo({ view: name, arg: arg ?? null });
    case "instructions": return goTo({ view: name, arg: arg ?? null });
    case "researchopen": return goTo({ view: name, arg });
  }
}

export function goTo(target: Location): boolean {
  const page = withReturnTarget(target, currentLocation());
  const { view: name, arg } = page;

  // A link to a page only a subscription has lands on Home, and a new or old chat on the
  // agent's start form.
  if (viewHidden(name, arg, state.subscribed, S.server)) {
    const chat = !!arg && typeof arg === "object" && "chat" in arg;
    return chat ? go("pane", { agentnew: { prompt: "" } }) : go("home");
  }

  if (route(windowScope(), page) === "main") {
    toMain(mainTarget(page));
    return false;
  }

  // Settings open over whatever shows, which stays as it is.
  if (name === "pane" && arg === "settings") {
    if (!currentPage) go("home");
    openSettings();
    return true;
  }

  const view = views.get(name);
  if (!view) return false;
  const afterMount = pageTiming.start(pageName(name, arg));
  // Sending from a draft turns it into the session; Back skips the spent form.
  const replaceDraft = currentPage?.view === "pane" && !!currentPage.arg && typeof currentPage.arg === "object" && "draft" in currentPage.arg
    && name === "pane" && !!arg && typeof arg === "object" && "agent" in arg && arg.adopt === true;
  const previous = currentLocation();
  const next: Location = page.view === "projects" ? { view: "projects", arg: { project: page.arg?.project ?? null, tile: page.arg?.tile ?? null } } : page;
  if (previous && !moving) pages.visit(previous, next, replaceDraft);
  if (previous && previous.view !== name) views.get(previous.view)?.unmount?.();
  currentPage = page;
  for (const [n, v] of views) v.el.hidden = n !== name;
  const mounted = view.mount(view.el, arg);
  refreshTopicSubscriptions();
  drawTop();
  drawSide();
  if (mounted) void mounted.then(() => afterMount(name === "pane" ? pageName(name, S.view) : undefined)).catch(e => console.warn("mounting page:", e));
  else afterMount();
  return true;
}

function withReturnTarget(page: Location, current: Location | null): Location {
  if (page.view !== "pane" || !opensAgent(page.arg) || page.arg.from || !current) return page;
  const target = page.arg;
  if (current.view !== "pane") return { view: "pane", arg: { ...target, from: current } };
  const previous = current.arg;

  if (previous && typeof previous === "object" && "draft" in previous) {
    return { view: "pane", arg: { ...target, from: { view: "projects", arg: { project: previous.draft.project } } } };
  }

  if (opensAgent(previous) && previous.from) return { view: "pane", arg: { ...target, from: previous.from } };
  return page;
}

let headerPage = "";
function patchHeader(host: HTMLElement, nodes: Node[], changedPage: boolean) {
  if (nodes.every((node): node is Element => node instanceof Element)) patchChildren(host, nodes, () => changedPage);
  else host.replaceChildren(...nodes);
}
export function drawTop() {
  const view = currentPage ? views.get(currentPage.view) : null;
  const head: ViewHead = typeof view?.title === "function" ? view.title() : { title: view?.title ?? "Beep" };
  // A project window's other views go back to the project.
  const back = document.getElementById("pw-back");
  if (back) back.hidden = currentPage?.view === "projects";
  $("#title").textContent = head.title || "Beep";
  const page = JSON.stringify([currentPage, currentPage?.view === "pane" ? [S.view, S.session, G.id] : null]);
  const changedPage = page !== headerPage;
  headerPage = page;
  // Header actions close over their page. Preserve them only within that page.
  patchHeader($("#scope"), head.scope ? [head.scope] : [], changedPage);
  $("#date").textContent = head.sub || "";
  patchHeader($("#tools"), head.tools || [], changedPage);
  syncMobile();
}
