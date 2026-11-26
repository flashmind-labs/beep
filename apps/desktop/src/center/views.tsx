import { Button, EmptyPanel } from "../ui/kit.tsx";
import { projectName } from "../project/projects-state.ts";
import { Fragment, memo, useEffect, useState } from "react";
import { flushSync } from "react-dom";
import { useStore } from "zustand";
import { refreshTopicSubscriptions } from "../ui/mesh-events.ts";
import { shownChanged } from "../ui/active.ts";
// The window's views: their boxes, going to one, and the header it draws.
import { recordTiming } from "../ui/tauri.ts";
import { perfOpened, perfView, perfViewOf } from "../ui/perf.ts";
import type { PaneTarget } from "../bar/pane.ts";
import { keepWindowSession, mainTarget, route, type Alone, type WindowScope } from "../project/projectwindow.ts";
import { ensureSessions, sessions, sessionsRead } from "../agents/sessions.ts";
import { threadsByChat } from "../agents/threads-state.ts";
import { shortName } from "../project/agent.ts";
import { type Location, type ViewHead, type CenterView, type ViewName } from "./state.ts";
import { drawSide } from "./sidebar.tsx";
import { openProjects } from "../project/addflow.ts";
import { projectRows } from "../project/projectrows.ts";
import { projectWindowProps } from "../project/projectops.ts";
import { instructionsView, openSettings, providersView } from "../settings/sheet.ts";
import { paneView } from "./pane.tsx";
import { homeView } from "./home.tsx";
import { TerminalsPage, terminalsView } from "../terminals/view.tsx";
import { boxOf, drawTop, toMain, go, goTo, keepBox, nav, pages, setWindowAlone, shown, windowAlone, aloneProject, currentLocation, headWanted, wanted, PAGES, type Page } from "./nav.ts";
import { PageTiming, pageName } from "./page-timing.ts";
import { S } from "../bar/core.ts";
import { isStarting } from "../agents/starting-state.ts";
import { canGoBack, syncMobile } from "./mobile.tsx";
import { placeWindowParts } from "./window-root.tsx";
import { closeSideOver, shell } from "./window-shell.tsx";
import { movePlaying, swipePlaying, type Swipe } from "../ui/motion.ts";

let loadedProjects: typeof import("../project/page.tsx") | null = null;
const projectsView: CenterView<"projects"> = {
  title: () => loadedProjects ? (typeof loadedProjects.projectsView.title === "function" ? loadedProjects.projectsView.title() : { title: loadedProjects.projectsView.title ?? "Projects" }) : { title: "Projects" },
  async mount(el, arg) {
    loadedProjects ??= await import("../project/page.tsx");
    nav.projectsLoaded = true;
    await loadedProjects.projectsView.mount(el, arg);
    if (nav.page?.view === "projects") drawTop();
  },
  unmount: () => loadedProjects?.projectsView.unmount?.(),
};

/// The view `name` is.
function viewOf(name: ViewName): CenterView {
  switch (name) {
    case "home": return homeView;
    case "pane": return paneView;
    case "projects": return projectsView;
    case "instructions": return instructionsView;
    case "providers": return providersView;
    case "terminals": return terminalsView;
  }
}

/// Every page's box, the one showing unhidden. Hidden pages stay drawn, so
/// coming back keeps their place.
function CenterPages() {
  const view = useStore(shell, s => s.view), drawn = useStore(shown, s => s.drawn);
  return PAGES.map(name => <PageBox key={name} name={name} hidden={view !== name} drawn={drawn[name]} />);
}

const PageBox = memo(function PageBox({ name, hidden, drawn }: { name: Page; hidden: boolean; drawn: number }) {
  return <div className="cc-box" hidden={hidden} ref={keepBox[name]}>{drawn ? <PageContent name={name} drawn={drawn} /> : null}</div>;
});

/// What page `name` draws, again only when it draws. Settings borrows
/// Instructions and Coding agents, so those draw into whichever host mounted them.
const PageContent = memo(function PageContent({ name }: { name: Page; drawn: number }) {
  switch (name) {
    case "projects": return loadedProjects ? <loadedProjects.ProjectsPage /> : null;
    case "terminals": return <TerminalsPage />;
    case "instructions": case "providers": return null;
  }
});

placeWindowParts({ pages: <CenterPages /> });

wanted.follow(target => { nav.opened = openPage(target); });
headWanted.follow(drawHead);

function show(name: ViewName) {
  flushSync(() => shell.setState({ view: name }));
}

/// A pane target that opens an agent view, which can go back.
function opensAgent(arg: PaneTarget): arg is Extract<PaneTarget, { agent: unknown } | { agentnew: unknown }> {
  return !!arg && typeof arg === "object" && ("agent" in arg || "agentnew" in arg);
}

export function openDetachedWindow(alone: Alone, saved: { machine_id: string; id: string } | null) {
  setWindowAlone(alone);
  document.body.classList.add("pw");

  const savedThread = alone.kind === "chat" && saved !== null
    && (saved.id !== alone.session || saved.machine_id !== alone.machine_id);

  const open = (chatOnly: boolean) => {
    const session = !chatOnly && saved && sessions.find(row => row.machine_id === saved.machine_id && row.id === saved.id);
    if (session) {
      const page = { view: "pane" as const, arg: { agent: session } };
      if (opensHere(page)) return goTo(page);
    }
    if (savedThread && !chatOnly && !session) return false;
    if (alone.kind === "project") return go("projects", { project: alone.project });
    const chat = sessions.find(row => row.machine_id === alone.machine_id && row.id === alone.session);
    return !!chat && go("pane", { agent: chat });
  };

  if (!open(false)) placeWindowParts({ title: document.title, opening: <WaitingForChat open={open} savedThread={savedThread} /> });
}

/// The machine can reconnect before it has published its sessions.
function WaitingForChat({ open, savedThread }: { open(chatOnly: boolean): boolean; savedThread: boolean }) {
  const [chatOnly, setChatOnly] = useState(false);

  useEffect(() => {
    let waiting = true;
    const check = () => { if (waiting && !currentLocation()) open(chatOnly); };
    sessionsRead.add(check);
    ensureSessions().then(check, () => {});

    return () => {
      waiting = false;
      sessionsRead.delete(check);
    };
  }, [open, chatOnly]);

  return <EmptyPanel spin text={savedThread && !chatOnly
    ? <>Waiting for the machine to list this thread… <Button onClick={() => setChatOnly(true)}>Open parent chat</Button></>
    : "Waiting for the machine to list this chat…"} />;
}

/// What a project's or chat's window keeps for itself; null in the main window.
function windowScope(): WindowScope | null {
  const alone = windowAlone;
  if (!alone) return null;
  if (alone.kind === "chat") {
    const chat = sessions.find(row => row.machine_id === alone.machine_id && row.id === alone.session);
    const threads = chat ? threadsByChat(sessions).get(chat)?.map(thread => thread.row) ?? [] : [];
    return { kind: "chat", sessions: [{ machine_id: alone.machine_id, id: alone.session }, ...threads] };
  }

  const p = (projectRows() || []).find(r => r.name === alone.project);
  return { kind: "project", project: alone.project, folders: p?.machines ?? [] };
}

/// A session may have moved out of this project or chat since it was saved.
function opensHere(page: Location): boolean {
  return route(windowScope(), page) === "here";
}


const pageTiming = new PageTiming(() => performance.now(), callback => { requestAnimationFrame(callback); }, () => !document.hidden, (page, ms) => { recordTiming("page", page, ms); perfOpened(perfViewOf(page), ms); });

/// Shows `target`; what `goTo` runs.
function openPage(target: Location): boolean {
  closeSideOver();
  const page = withReturnTarget(target, currentLocation());
  const { view: name, arg } = page;

  if (!opensHere(page)) {
    toMain(mainTarget(page));
    return false;
  }

  // Settings open over whatever shows, which stays as it is.
  if (name === "pane" && arg === "settings") {
    if (!nav.page) go("home");
    openSettings();
    return true;
  }

  const view = viewOf(name), box = boxOf(name);
  if (!box) return false;
  const afterMount = pageTiming.start(pageName(name, arg));
  // Sending from a draft turns it into the session, and a start becomes the session its
  // machine names: Back skips the spent page.
  const spent = nav.page?.view === "pane" && !!nav.page.arg && typeof nav.page.arg === "object"
    && ("draft" in nav.page.arg || ("agent" in nav.page.arg && isStarting(nav.page.arg.agent.id)));
  const replaceSpent = spent && name === "pane" && !!arg && typeof arg === "object" && "agent" in arg && arg.adopt === true;
  const previous = currentLocation();
  const next: Location = page.view === "projects" ? { view: "projects", arg: { project: page.arg?.project ?? null, tile: page.arg?.tile ?? null } } : page;
  if (!nav.moving) { if (previous) pages.visit(previous, next, replaceSpent); else pages.start(next); }
  if (previous && previous.view !== name) viewOf(previous.view).unmount?.();
  nav.page = page;
  if (windowAlone) keepWindowSession(page);
  perfView(perfViewOf(pageName(name, arg)));
  show(name);
  const mounted = view.mount(box, arg);
  refreshTopicSubscriptions();
  shownChanged();
  drawTop();
  drawSide();
  if (previous && JSON.stringify(previous) !== JSON.stringify(next)) animateIn(box, nav.goingBack);
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

const MOTION: Record<PageMotion | Swipe, { frames: Keyframe[]; ms: number }> = {
  // Opacity only, so popovers measured while it plays land where they belong. It starts
  // half drawn and short, so the page reads as there at once.
  fade: { frames: [{ opacity: .5 }, { opacity: 1 }], ms: 100 },
  push: { frames: [{ opacity: .6, transform: "translateX(28%)" }, { opacity: 1, transform: "none" }], ms: 240 },
  pop: { frames: [{ opacity: .6, transform: "translateX(-22%)" }, { opacity: 1, transform: "none" }], ms: 220 },
  // A swiped page comes in from the side the last one left room on.
  back: { frames: [{ opacity: .3, transform: "translateX(-56px)" }, { opacity: 1, transform: "none" }], ms: 200 },
  forward: { frames: [{ opacity: .3, transform: "translateX(56px)" }, { opacity: 1, transform: "none" }], ms: 200 },
};
const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)");

/// Plays the page's way in on its box; the compositor runs it, so the page draws as usual.
function animateIn(box: HTMLElement, back: boolean) {
  const move = movePlaying();
  // The next conversation slides in as a captured page.
  if (reduceMotion.matches || document.hidden || move === "next") return;
  const phone = document.body.classList.contains("mobile");
  // A card growing into the head, or back, carries the eye: the page under it only fades.
  const { frames, ms } = MOTION[swipePlaying() ?? (move ? "fade" : pageMotion(phone, back, phone && canGoBack()))];
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

/// Draws the head of the page showing; what `drawTop` runs.
function drawHead() {
  const view = nav.page ? viewOf(nav.page.view) : null;
  const head: ViewHead = typeof view?.title === "function" ? view.title() : { title: view?.title ?? "Beep" };
  const project = aloneProject(), alone = windowAlone;
  const row = projectRows()?.find(row => row.name === project);
  const label = row ? projectName(row) : project;
  if (project) document.title = label ?? project;
  if (alone?.kind === "chat") document.title = sessions.find(row => row.machine_id === alone.machine_id && row.id === alone.session)?.title || "Assistant";
  const page = JSON.stringify([nav.page, nav.page?.view === "pane" ? S.view : null]);
  placeWindowParts({
    "project-back": project ? <Button {...projectWindowProps(project).clicks} {...projectWindowProps(project).menu} small className="pw-back" id="pw-back" title={`Back to ${label} (esc)`} hidden={nav.page?.view === "projects"} onClick={() => void openProjects(project)}>{shortName(label ?? project)}</Button> : null,
    title: head.title || "Beep",
    date: head.sub || "",
    scope: head.reactScope ? <Fragment key={page}>{head.reactScope}</Fragment> : null,
    tools: head.reactTools ? <Fragment key={page}>{head.reactTools}</Fragment> : null,
  });
  syncMobile();
}
