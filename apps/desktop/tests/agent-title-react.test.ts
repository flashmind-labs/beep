import { bar } from "./steer-env.ts";
import test from "node:test";
import assert from "node:assert/strict";

const { createElement } = await import("react");
const { flushSync } = await import("react-dom");
const { createRoot } = await import("react-dom/client");
const { fireEvent } = await import("@testing-library/react");
const { SessionTitle } = await import("../src/parts/session/title-view.tsx");
const settle = () => new Promise<void>(resolve => setImmediate(resolve));

test("a changed session title does not replace the draft in its editor", async () => {
  const host = document.createElement("div"); bar.append(host);
  const root = createRoot(host);
  let saved = "";
  const save = async (value: string) => { saved = value; };
  flushSync(() => root.render(createElement(SessionTitle, { title: "Original", save })));
  const title = host.querySelector<HTMLElement>("#agtitle"); assert.ok(title);
  flushSync(() => fireEvent.keyDown(title, { key: "Enter" }));
  const input = host.querySelector<HTMLInputElement>("input"); assert.ok(input);
  assert.equal(document.activeElement, input);
  flushSync(() => fireEvent.change(input, { target: { value: "My draft" } }));
  flushSync(() => root.render(createElement(SessionTitle, { title: "Remote update", save })));
  assert.equal(host.querySelector("input"), input);
  assert.equal(input.value, "My draft");
  flushSync(() => fireEvent.keyDown(input, { key: "Enter" }));
  await settle();
  assert.equal(saved, "My draft");
  assert.equal(host.querySelector("input"), null);
  assert.equal(document.activeElement, title);
  root.unmount(); host.remove();
});

test("a session title without save cannot be renamed", () => {
  const host = document.createElement("div"); bar.append(host);
  const root = createRoot(host);
  flushSync(() => root.render(createElement(SessionTitle, { title: "Review #41", save: null })));
  const title = host.querySelector<HTMLElement>("#agtitle"); assert.ok(title);
  flushSync(() => fireEvent.keyDown(title, { key: "Enter" }));
  flushSync(() => fireEvent.doubleClick(title));
  assert.equal(host.querySelector("input"), null);
  assert.equal(title.getAttribute("role"), null);
  root.unmount(); host.remove();
});
