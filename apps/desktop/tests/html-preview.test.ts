import test from "node:test";
import assert from "node:assert/strict";
import { registerHooks } from "node:module";
import { JSDOM } from "jsdom";
import { isHtmlFile } from "../src/agentpane/html-preview-state.ts";

const dom = new JSDOM("<body></body>");
Object.assign(globalThis, { window: dom.window, document: dom.window.document, HTMLElement: dom.window.HTMLElement, Node: dom.window.Node });
Object.defineProperty(globalThis, "navigator", { configurable: true, value: dom.window.navigator });
const { createRoot } = await import("react-dom/client");
const { flushSync } = await import("react-dom");
const { createElement } = await import("react");
const state = { session: { machine_id: "remote", cwd: "/srv/project" }, tabs: [] as any[] };
const calls: unknown[] = [];
let answer: () => Promise<unknown> = async () => ({ kind: "html", src: "beepview://localhost/test/" });
Object.assign(globalThis, { previewState: state, previewInvoke: async (cmd: string, args: unknown) => { if (cmd === "editor_settings_get") return { vim: null }; calls.push({ cmd, args }); return answer(); } });
const stubs = new Map(Object.entries({
  "agentpane/pane-tabs.ts": "export const touchTab = t => globalThis.previewDraw(t); export const noteTab = () => {}; export const discardSheet = () => {}; export const closeTab = () => {}; export const leavePane = () => {};",
  "agentpane/media-layout.ts": "export const openTabs = () => globalThis.previewState.tabs;",
  "agentpane/pane-memory.ts": "export const rememberPane = () => {}; export const rememberTerminal = () => {};",
  "agentpane/pane-state.ts": "export const P = globalThis.previewState; export const redraw = () => {}; export const refollow = () => {}; export const newFileEdit = base => ({ base, view: null, state: null, generation: 0, dirty: false, saving: false, conflict: false, at: null });",
  "agentpane/pane-open.ts": "export const openLocation = () => {};",
  "agentpane/terminal.ts": "export const attach = () => {}; export const detachTerm = () => {}; export const killTerm = () => {}; export const dropTerm = () => {}; export const followTerm = () => {}; export const forgetTerm = () => {}; export const openTerm = () => {};",
  "ui/tauri.ts": "export const invoke = globalThis.previewInvoke; export const listen = async () => () => {};",
  "chat/media.tsx": "export const fileSize = () => ''; export const openViewer = () => {};",
  "ui/active.ts": "export const paneShown = () => true;",
  "ui/menu.tsx": "export const confirmSheet = () => {}; export const openMenu = () => {};",
  "agents/mentions.ts": "export const addMention = () => {};",
}).map(([path, source]) => [new URL(`../src/${path}`, import.meta.url).href, source]));
const hook = registerHooks({ load(url, context, next) {
  const source = stubs.get(url);
  return source === undefined ? next(url, context) : { format: "module", source, shortCircuit: true };
} });
const { TextView } = await import("../src/agentpane/text-view.tsx");
hook.deregister();
const tick = () => new Promise(r => setImmediate(r));
const roots = new Map<unknown, { root: ReturnType<typeof createRoot>; rev: number }>();
function draw(t: any) {
  const shown = roots.get(t);
  if (!shown) return;
  shown.rev++;
  flushSync(() => shown.root.render(createElement(TextView, { tab: t, text: t.text })));
}
Object.assign(globalThis, { previewDraw: draw });
function tab(name = "report.HTML") {
  const t: any = { id: name, kind: "text", rel: name, root: "/srv/project/work", name, path: null, size: 30, text: { text: "<h1>Hello</h1>", size: 30, binary: false, cut: false } };
  state.tabs = [t];
  t.el = document.createElement("div");
  document.body.replaceChildren(t.el);
  roots.set(t, { root: createRoot(t.el), rev: 0 });
  draw(t);
  return t;
}

test("HTML detection includes htm and uppercase names", () => {
  assert.ok(isHtmlFile("page.HTM"));
  assert.ok(isHtmlFile("page.HTML"));
  assert.ok(!isHtmlFile("page.html.txt"));
});

test("remote HTML offers preview and returns to source", async () => {
  const t = tab();
  (t.el.querySelector("[data-preview]") as HTMLButtonElement).click();
  await tick();
  assert.deepEqual(calls.at(-1), { cmd: "agent_artifact", args: { machineId: "remote", root: "/srv/project/work", path: "report.HTML", preview: true } });
  const frame = t.el.querySelector("iframe");
  assert.equal(frame.getAttribute("sandbox"), "allow-scripts");
  assert.equal(frame.getAttribute("referrerpolicy"), "no-referrer");
  assert.equal(frame.src, "beepview://localhost/test/");
  t.el.querySelector("[data-preview]").click();
  await tick();
  assert.ok(t.el.querySelector(".agcode"));
  assert.equal(t.el.querySelector("iframe"), null);
  assert.equal(tab("notes.txt").el.querySelector("[data-preview]"), null);
});

test("a failed read leaves source accessible and preview can be retried", async () => {
  answer = async () => { throw new Error("Machine offline"); };
  const t = tab();
  t.el.querySelector("[data-preview]").click();
  await tick();
  assert.match(t.el.textContent, /Machine offline/);
  t.el.querySelector("[data-preview]").click();
  await tick();
  assert.ok(t.el.querySelector(".agcode"));
  answer = async () => ({ kind: "html", src: "beepview://localhost/retry/" });
  t.el.querySelector("[data-preview]").click();
  await tick();
  await tick();
  assert.equal(t.el.querySelector("iframe").src, "beepview://localhost/retry/");
});

test("returning to source while the preview loads keeps the source visible", async () => {
  let finish!: (value: unknown) => void;
  answer = () => new Promise(resolve => { finish = resolve; });
  const t = tab();
  t.el.querySelector("[data-preview]").click();
  await tick();
  assert.match(t.el.textContent, /Loading preview/);
  t.el.querySelector("[data-preview]").click();
  await tick();
  finish({ kind: "html", src: "beepview://localhost/late/" });
  await tick();
  assert.ok(t.el.querySelector(".agcode"));
  assert.equal(t.el.querySelector("iframe"), null);
});

test("Markdown previews as rendered text and returns to source", async () => {
  answer = async () => ({ kind: "md", html: "<h1>Hello</h1>" });
  const t = tab("README.md");
  t.el.querySelector("[data-preview]").click();
  await tick();
  assert.equal(t.el.querySelector(".agartmd h1").textContent, "Hello");
  assert.equal(t.el.querySelector("iframe"), null);
  t.el.querySelector("[data-preview]").click();
  await tick();
  assert.ok(t.el.querySelector(".agcode"));
});
