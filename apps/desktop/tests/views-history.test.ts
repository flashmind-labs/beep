import test from "node:test";
import assert from "node:assert/strict";
import { registerHooks } from "node:module";
import { JSDOM } from "jsdom";
import type { CenterView, ViewArg, ViewName } from "../src/center/state.ts";

// Load the real navigation with the window's DOM and services replaced.
const dom = new JSDOM("<!doctype html><body><div class='cwin'></div></body>");
// jsdom has no media queries; Reduce motion keeps page animations out of these tests.
dom.window.matchMedia = query => ({ matches: query.includes("reduce"), media: query } as MediaQueryList);
const state = { picked: null as string | null, subscribed: false, agents: [] };
const views = new Map<ViewName, CenterView>();
/// The view each page module exports, calling what a test registered for it.
const view = (name: ViewName): CenterView => ({
  mount: (el, arg) => views.get(name)?.mount(el, arg),
  unmount: () => views.get(name)?.unmount?.(),
  currentArg: () => views.get(name)?.currentArg?.() ?? null,
});
const register = <K extends ViewName>(name: K, page: CenterView<K>) => { views.set(name, page); };
Object.assign(globalThis, {
  navigationTest: { state, view },
  window: dom.window, document: dom.window.document, HTMLElement: dom.window.HTMLElement, Element: dom.window.Element, Node: dom.window.Node,
  requestAnimationFrame: () => 0,
  // jsdom lays nothing out, so nothing is ever resized.
  ResizeObserver: class { observe() {} disconnect() {} },
  IntersectionObserver: class { observe() {} disconnect() {} },
});
const page = (exports: string) => `const v = globalThis.navigationTest.view; ${exports}`;
const stubs = new Map(Object.entries({
  "center/state.ts": "export const { state } = globalThis.navigationTest; export const $ = selector => document.querySelector(selector);",
  "ui/tauri.ts": "export const invoke = async () => {}; export const recordTiming = () => {}; export const buildCapabilities = async () => ({ metrics_recording: false });",
  "ui/mesh-events.ts": "export const refreshTopicSubscriptions = () => {};",
  "ui/active.ts": "export const SLOW_MS = 2000; export const shownChanged = () => {}; export const pageShown = () => true;",
  "center/sidebar.tsx": "export const drawSide = () => {};",
  "project/addflow.ts": "export const openProjects = () => {};",
  "project/projectops.ts": "export const projectWindowProps = () => ({ clicks: {}, menu: {} });",
  "project/projectrows.ts": "export const projectRows = () => [];",
  "project/projects-state.ts": "export const projectName = project => project.display_name ?? project.name;",
  "project/agent.ts": "export const shortName = name => name;",
  "settings/sheet.ts": page("export const openSettings = () => {}; export const instructionsView = v('instructions'); export const providersView = v('providers');"),
  "center/pane.tsx": page("export const paneView = v('pane');"),
  "center/home.tsx": page("export const homeView = v('home');"),
  "project/page.tsx": page("export const projectsView = v('projects'); export const ProjectsPage = () => null;"),
  "terminals/view.tsx": page("export const terminalsView = v('terminals'); export const TerminalsPage = () => null;"),
  "bar/core.ts": "export const S = { view: null };",
  "agents/sessions.ts": "export const sessions = []; export const loadSessions = async () => sessions; export const ensureSessions = loadSessions; export const sessionsRead = new Set();",
  "center/mobile.tsx": "export const syncMobile = () => {}; export const canGoBack = () => false;",
  // The window's tree without the router.
  "center/window-root.tsx": `import { createElement } from "react"; import { flushSync } from "react-dom"; import { createRoot } from "react-dom/client";
    import { CenterWindow, shell } from "./window-shell.tsx";
    const root = createRoot(document.querySelector(".cwin"));
    flushSync(() => root.render(createElement(CenterWindow)));
    export const placeWindowParts = parts => flushSync(() => shell.setState(s => ({ parts: { ...s.parts, ...parts } })));`,
}).map(([path, source]) => [new URL(`../src/${path}`, import.meta.url).href, source]));
const hooks = registerHooks({
  load(url, context, next) {
    const source = stubs.get(url);
    return source === undefined ? next(url, context) : { format: "module", source, shortCircuit: true };
  },
});
await import("../src/center/views.tsx");
const { go, goHistory } = await import("../src/center/nav.ts");
// The router loads the project page on its first visit.
await import("../src/project/page.tsx");
hooks.deregister();

function projectMounted(page: CenterView<"projects">): Promise<void> {
  return new Promise(resolve => register("projects", {
    ...page, mount(el, arg) { const result = page.mount(el, arg); resolve(); return result; },
  }));
}

test("Back and Forward restore the project section a resumed session came from", async () => {
  let tile: string | null = null;
  const mounted: { arg: ViewArg } = { arg: null };
  register("home", { mount() {} });
  const ready = projectMounted({
    currentArg: () => ({ project: state.picked, tile }),
    mount(_el, arg) {
      state.picked = arg?.project ?? null;
      tile = arg?.tile ?? null;
    },
    unmount() { tile = null; },
  });
  register("pane", { mount(_el, arg) { mounted.arg = arg; } });

  go("home");
  go("projects", { project: "acme/widget" });
  await ready;
  tile = "running";
  const session = { agent: { machine_id: "build-box", machine_name: "build-box", id: "session", cwd: "/srv/project", agent: "codex", status: "idle", external: false } };
  go("pane", session);
  assert.equal(tile, null, "leaving the project clears its live selection");
  assert.deepEqual(returnTarget(mounted.arg), {
    view: "projects", arg: { project: "acme/widget", tile: "running" },
  });
  goHistory(false);
  assert.equal(state.picked, "acme/widget");
  assert.equal(tile, "running");
  goHistory(true);
  assert.ok(mounted.arg && typeof mounted.arg === "object" && "agent" in mounted.arg);
  goHistory(false);
  assert.equal(tile, "running");
  goHistory(false);
  assert.equal(dom.window.document.getElementById("home-box")?.hidden, false, "the section adds no extra history step");
  goHistory(true);
  assert.equal(tile, "running");

  go("projects", { project: "acme/other" });
  assert.equal(tile, null, "opening another project starts at Overview");
  goHistory(false);
  assert.equal(state.picked, "acme/widget");
  assert.equal(tile, "running");
});

test("notifications keep the project return target across conversations", async () => {
  const mounted: { arg: ViewArg } = { arg: null };
  register("pane", { mount(_el, arg) { mounted.arg = arg; } });
  const agent = { machine_id: "build-box", machine_name: "build-box", id: "notice", cwd: "/srv/project", agent: "codex", status: "idle", external: false };
  const project = { project: "acme/widget", tile: "running" };
  const ready = projectMounted({ currentArg: () => project, mount() {} });
  go("projects", project);
  await ready;
  go("pane", { agent });
  go("pane", { agent: { ...agent, id: "second-notice" } });
  assert.deepEqual(returnTarget(mounted.arg), { view: "projects", arg: project });
});

test("a notification from the project composer returns to the project", () => {
  const mounted: { arg: ViewArg } = { arg: null };
  register("pane", { mount(_el, arg) { mounted.arg = arg; } });
  go("pane", { draft: { project: "acme/widget", machine: "build-box" } });
  go("pane", { agent: { machine_id: "build-box", machine_name: "build-box", id: "notice", cwd: "/srv/project", agent: "codex", status: "idle", external: false } });
  assert.deepEqual(returnTarget(mounted.arg), { view: "projects", arg: { project: "acme/widget" } });
});

test("explicit return targets take priority when opening a conversation", () => {
  const mounted: { arg: ViewArg } = { arg: null };
  register("pane", { mount(_el, arg) { mounted.arg = arg; } });
  go("pane", { draft: { project: "acme/widget", machine: null } });
  const from = { view: "home" as const, arg: null };
  go("pane", { agentnew: { prompt: "", assistant: false }, from });
  assert.deepEqual(returnTarget(mounted.arg), from);
});

function returnTarget(arg: ViewArg) {
  return arg && typeof arg === "object" && "from" in arg ? arg.from : null;
}
