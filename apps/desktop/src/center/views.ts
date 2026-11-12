import { createElement, Fragment, memo } from "react";
import { flushSync } from "react-dom";
import { createStore, useStore } from "zustand";
import { refreshTopicSubscriptions } from "../ui/mesh-events.ts";
import { shownChanged } from "../ui/active.ts";
// The window's views: their boxes, going to one, and the header it draws.
import { invoke, recordTiming } from "../ui/tauri.ts";
import { perfOpened, perfView, perfViewOf } from "../ui/perf.ts";
import type { PaneTarget } from "../bar/pane.ts";
import { mainTarget, route, type CenterTarget, type ProjectScope } from "../project/projectwindow.ts";
import { chatProject, shortName } from "../project/agent.ts";
import { type Location, type NavigationArgs, type ViewHead, type CenterView, type ViewName, state } from "./state.ts";
import { drawSide, openProjects, projectRows } from "./sidebar.ts";
import { instructionsView, openSettings, providersView } from "../settings/sheet.ts";
import { paneView, threadChats } from "./threadcard.ts";
import { homeView } from "./home.ts";
import { ProjectsPage, projectsView } from "../project/page.ts";
import { SyncRoute, syncView } from "../project/syncpage.ts";
import { ResearchList, researchView } from "../research/page.ts";
import { ResearchOpenPage, researchOpenView } from "../research/open.ts";
import { TerminalsPage, terminalsView } from "../terminals/view.ts";
import { windowHistory } from "./window-router.ts";
import { PageTiming, pageName } from "./page-timing.ts";
import { viewHidden } from "../settings/subscription.ts";
import { G, S } from "../bar/core.ts";
import { canGoBack, syncMobile } from "./mobile.ts";
import { placeWindowParts } from "./window-root.ts";
import { shell, windowHost } from "./window-shell.tsx";
import { movePlaying } from "../ui/motion.ts";

let currentPage: Location | null = null;

/// The view `name` is.
function viewOf(name: ViewName): CenterView {
  switch (name) {
    case "home": return homeView;
    case "pane": return paneView;
    case "projects": return projectsView;
    case "sync": return syncView;
    case "instructions": return instructionsView;
    case "providers": return providersView;
    case "research": return researchView;
    case "researchopen": return researchOpenView;
    case "terminals": return terminalsView;
  }
}

/// The pages after Home and the pane, in the order their boxes follow them in `#main`.
const PAGES = ["projects", "sync", "instructions", "providers", "research", "researchopen", "terminals"] as const;
type Page = typeof PAGES[number];
const boxes = new Map<Page, HTMLElement>();
const keepBox = Object.fromEntries(PAGES.map(name => [name, (el: HTMLDivElement | null) => { if (el) boxes.set(name, el); else boxes.delete(name); }]));
/// How often each page drew; 0 leaves its box empty.
const shown = createStore<{ drawn: Record<Page, number> }>(() => ({
  drawn: { projects: 0, sync: 0, instructions: 0, providers: 0, research: 0, researchopen: 0, terminals: 0 },
}));

/// Draws page `name` in its box again, or empties the box.
export function drawPage(name: Page, on = true) {
  flushSync(() => shown.setState(s => ({ drawn: { ...s.drawn, [name]: on ? s.drawn[name] + 1 : 0 } })));
}

/// Every page's box, the one showing unhidden. Hidden pages stay drawn, so
/// coming back keeps their place.
function CenterPages() {
  const view = useStore(shell, s => s.view), drawn = useStore(shown, s => s.drawn);
  return PAGES.map(name => createElement(PageBox, { key: name, name, hidden: view !== name, drawn: drawn[name] }));
}

const PageBox = memo(function PageBox({ name, hidden, drawn }: { name: Page; hidden: boolean; drawn: number }) {
  return createElement("div", { className: "cc-box", hidden, ref: keepBox[name] }, drawn ? createElement(PageContent, { name, drawn }) : null);
});

/// What page `name` draws, again only when it draws. Settings borrows
/// Instructions and Coding agents, so those draw into whichever host mounted them.
const PageContent = memo(function PageContent({ name }: { name: Page; drawn: number }) {
  switch (name) {
    case "projects": return createElement(ProjectsPage);
    case "sync": return createElement(SyncRoute);
    case "research": return createElement(ResearchList);
    case "researchopen": return createElement(ResearchOpenPage);
    case "terminals": return createElement(TerminalsPage);
    case "instructions": case "providers": return null;
  }
});

placeWindowParts({ pages: createElement(CenterPages) });

/// The element view `name` draws in; Home and the pane have theirs in the page.
export function boxOf(name: ViewName): HTMLElement | null {
  switch (name) {
    case "home": return windowHost("home-box");
    case "pane": return windowHost("pane-box");
    default: return boxes.get(name) ?? null;
  }
}

function show(name: ViewName) {
  flushSync(() => shell.setState({ view: name }));
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

const pages = windowHistory;
/// Set while ⌘[ or ⌘] moves, so the move is not a new visit.
let moving = false;
/// Set while a Back moves, so the page comes in from the left on the phone.
let goingBack = false;

/// Runs `step` as a Back: the phone's Back goes up a level without a history step.
export function asBack(step: () => void) {
  goingBack = true;

  try {
    step();
  } finally {
    goingBack = false;
  }
}
const pageTiming = new PageTiming(() => performance.now(), callback => { requestAnimationFrame(callback); }, () => !document.hidden, (page, ms) => { recordTiming("page", page, ms); perfOpened(perfViewOf(page), ms); });

/// The page showing, as `go` opens it again.
export function currentLocation(): Location | null {
  if (!currentPage) return null;

  // Project sections change without mounting the page again.
  if (currentPage.view === "projects") {
    const arg = projectsView.currentArg?.();
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
    if (forward) goTo(page); else asBack(() => goTo(page));
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

  const view = viewOf(name), box = boxOf(name);
  if (!box) return false;
  const afterMount = pageTiming.start(pageName(name, arg));
  // Sending from a draft turns it into the session; Back skips the spent form.
  const replaceDraft = currentPage?.view === "pane" && !!currentPage.arg && typeof currentPage.arg === "object" && "draft" in currentPage.arg
    && name === "pane" && !!arg && typeof arg === "object" && "agent" in arg && arg.adopt === true;
  const previous = currentLocation();
  const next: Location = page.view === "projects" ? { view: "projects", arg: { project: page.arg?.project ?? null, tile: page.arg?.tile ?? null } } : page;
  if (!moving) { if (previous) pages.visit(previous, next, replaceDraft); else pages.start(next); }
  if (previous && previous.view !== name) viewOf(previous.view).unmount?.();
  currentPage = page;
  perfView(perfViewOf(pageName(name, arg)));
  show(name);
  const mounted = view.mount(box, arg);
  refreshTopicSubscriptions();
  shownChanged();
  drawTop();
  drawSide();
  if (previous && JSON.stringify(previous) !== JSON.stringify(next)) animateIn(box, goingBack);
  if (mounted) void mounted.then(() => afterMount(name === "pane" ? pageName(name, S.view) : undefined)).catch(e => console.warn("mounting page:", e));
  else afterMount();
  return true;
}

type PageMotion = "fade" | "push" | "pop";

/// How a page comes in: the phone slides a page in over its tab and back out the other
/// way, and fades between tabs; the desktop fades.
function pageMotion(phone: boolean, back: boolean, hasBack: boolean): PageMotion {
  if (!phone) return "fade";
  if (back) return "pop";
  return hasBack ? "push" : "fade";
}

const MOTION: Record<PageMotion, { frames: Keyframe[]; ms: number }> = {
  // Opacity only, so popovers measured while it plays land where they belong. It starts
  // half drawn and short, so the page reads as there at once.
  fade: { frames: [{ opacity: .5 }, { opacity: 1 }], ms: 100 },
  push: { frames: [{ opacity: .6, transform: "translateX(28%)" }, { opacity: 1, transform: "none" }], ms: 240 },
  pop: { frames: [{ opacity: .6, transform: "translateX(-22%)" }, { opacity: 1, transform: "none" }], ms: 220 },
};
const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)");

/// Plays the page's way in on its box; the compositor runs it, so the page draws as usual.
function animateIn(box: HTMLElement, back: boolean) {
  // A move already carries the page in.
  if (reduceMotion.matches || document.hidden || movePlaying()) return;
  const phone = document.body.classList.contains("mobile");
  const { frames, ms } = MOTION[pageMotion(phone, back, phone && canGoBack())];
  for (const running of box.getAnimations()) running.cancel();
  box.animate(frames, { duration: ms, easing: "cubic-bezier(.2, .8, .2, 1)" });
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

export function drawTop() {
  const view = currentPage ? viewOf(currentPage.view) : null;
  const head: ViewHead = typeof view?.title === "function" ? view.title() : { title: view?.title ?? "Beep" };
  const project = windowProject;
  const page = JSON.stringify([currentPage, currentPage?.view === "pane" ? [S.view, S.session, G.id] : null]);
  placeWindowParts({
    "project-back": project ? createElement("button", { type: "button", className: "dbtn small pw-back", id: "pw-back", title: `Back to ${project} (esc)`, hidden: currentPage?.view === "projects", onClick: () => void openProjects(project) }, shortName(project)) : null,
    title: head.title || "Beep",
    date: head.sub || "",
    scope: head.reactScope ? createElement(Fragment, { key: page }, head.reactScope) : null,
    tools: head.reactTools ? createElement(Fragment, { key: page }, head.reactTools) : null,
  });
  syncMobile();
}
