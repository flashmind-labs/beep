import { answers, calls } from "./dom-env.ts";
import test, { afterEach, beforeEach, mock } from "node:test";
import assert from "node:assert/strict";
import { fileLinkMenu } from "../src/agents/filelinks.ts";
import { closeMenu } from "../src/ui/menu.ts";

const folder = { machineId: "build-box-id", root: "/srv/project" };
const settle = () => new Promise(resolve => setImmediate(resolve));
let listeners: WeakMap<object, Map<string, (e: unknown) => void>>;

beforeEach(() => {
  calls.length = 0;
  answers.local_file = { apps: [] };
  listeners = new WeakMap();
  const proto = Object.getPrototypeOf(document.createElement("div"));
  mock.method(proto, "addEventListener", function(this: object, kind: string, fn: (e: unknown) => void) {
    if (!listeners.has(this)) listeners.set(this, new Map());
    listeners.get(this)!.set(kind, fn);
  });
  Object.assign(globalThis, { innerWidth: 1200, innerHeight: 900, removeEventListener() {} });
});
afterEach(() => { closeMenu(); mock.restoreAll(); });

function context(href: string, open: (path: string) => void = () => {}) {
  const link = document.createElement("a");
  link.setAttribute("href", href);
  const code = document.createElement("code");
  link.append(code);
  const event = { target: code, clientX: 20, clientY: 30, preventDefault() {}, stopPropagation() {} } as unknown as MouseEvent;
  fileLinkMenu(event, folder, open);
}

function click(label: string) {
  const button = [...document.querySelectorAll(".cc-menu button")].find(b => b.textContent === label);
  assert.ok(button, `Menu has ${label}`);
  listeners.get(button)?.get("click")?.({});
}

const labels = () => [...document.querySelectorAll(".cc-menu button")].map(b => b.textContent);

test("a reply's mockup opens in its default app using the session's machine and folder", async () => {
  context("mockups/desktop/index.html");
  await settle();
  assert.deepEqual(labels(), ["Open in Beep", "Copy path", "Open in default app", "Reveal in Finder"]);
  click("Open in default app");
  await settle();
  assert.deepEqual(calls.find(c => c.cmd === "open_local_file")?.args, {
    ...folder, path: "mockups/desktop/index.html", action: "open",
  });
});

test("absolute references, spaces, and source line suffixes resolve to a relative local file", async () => {
  context("/srv/project/mockups/../mockups/my%20page.html:12:3");
  await settle();
  assert.deepEqual(calls.find(c => c.cmd === "local_file")?.args, { ...folder, path: "mockups/my page.html" });
});

test("Open in Beep works even when a file has no local copy", async () => {
  answers.local_file = null;
  let opened = "";
  context("mockups/index.html#L12", path => { opened = path; });
  await settle();
  assert.deepEqual(labels(), ["Open in Beep", "Copy path", "Open in default app"]);
  click("Open in Beep");
  assert.equal(opened, "mockups/index.html");
});

test("web links and page anchors do not request local file actions", () => {
  for (const href of ["https://example.com/mockup.html", "#section", "mailto:user@example.com"]) context(href);
  assert.deepEqual(labels(), []);
  assert.equal(calls.length, 0);
});

test("paths outside the session folder do not request native actions", async () => {
  for (const href of ["../other/mockup.html", "/srv/project-other/mockup.html"]) {
    context(href);
    await settle();
    assert.deepEqual(labels(), ["Open in Beep", "Copy path", "Open in default app"]);
    closeMenu();
  }

  assert.equal(calls.length, 0);
});

test("a late app lookup cannot reopen a dismissed menu", async () => {
  let finish!: (value: unknown) => void;
  answers.local_file = new Promise(resolve => { finish = resolve; });
  context("mockups/index.html");
  closeMenu();
  finish({ apps: [] });
  await settle();
  assert.deepEqual(labels(), []);
});

test("a remote file opens through the default app", async () => {
  answers.local_file = null;
  context("mockups/index.html");
  await settle();
  click("Open in default app");
  await settle();
  assert.deepEqual(calls.find(c => c.cmd === "agent_file_open")?.args, {
    machineId: folder.machineId, path: "/srv/project/mockups/index.html",
  });
});
