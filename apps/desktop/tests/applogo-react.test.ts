import { answers, bar } from "./chat-dom-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { AppLogo } from "../src/chat/applogo-view.tsx";
import type { AppLogo as AppLogoRecord } from "../src/chat/approvals.ts";

Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { value: true, configurable: true });

test("app logos ignore stale reads and fall back after an image error", async () => {
  let finish: (items: AppLogoRecord[]) => void = () => {};
  answers.list_apps = new Promise<AppLogoRecord[]>(resolve => { finish = resolve; });
  const host = document.createElement("div");
  bar.append(host);
  const root = createRoot(host);
  await act(() => root.render(createElement(AppLogo, { app: "First" })));
  await act(() => root.render(createElement(AppLogo, { app: "Second" })));
  await act(async () => { finish([{ slug: "first", name: "First", logo_url: "https://example.test/first.png" }, { slug: "second", name: "Second", logo_url: "https://example.test/second.png" }]); });
  const tile = host.querySelector(".dapp"), image = tile?.querySelector("img");
  assert.equal(tile?.getAttribute("data-app"), "Second");
  assert.equal(image?.getAttribute("src"), "https://example.test/second.png");
  assert.ok(image);
  await act(() => { image.dispatchEvent(new window.Event("error")); });
  assert.equal(tile?.textContent, "S");
  assert.equal(tile?.classList.contains("logo"), false);
  await act(() => root.unmount());
  host.remove();
});
