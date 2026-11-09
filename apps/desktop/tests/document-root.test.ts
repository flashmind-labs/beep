import "./git-history-env.ts";
import { createContext, createElement, useContext } from "react";
import { flushSync } from "react-dom";
import test from "node:test";
import assert from "node:assert/strict";
import { createDocumentScope, createDocumentView } from "../src/ui/document-root.tsx";

test("document scopes synchronously update only their own portals and dispose independently", () => {
  const context = createContext("missing"), first = document.createElement("div"), second = document.createElement("div");
  document.body.append(first, second);
  let firstRenders = 0, secondRenders = 0;
  function First({ text }: { text: string }) { firstRenders++; return createElement("span", null, useContext(context), text); }
  function Second() { secondRenders++; return createElement("input", { defaultValue: "draft" }); }
  const a = createDocumentScope<"item">(children => createElement(context.Provider, { value: "context" }, children));
  const b = createDocumentScope<"item">();
  try {
    a.set([{ slot: "item", host: first, content: createElement(First, { text: " one" }) }]);
    b.mount();
    flushSync(() => b.setDeferred([{ slot: "item", host: second, content: createElement(Second) }]));
    const input = second.querySelector("input");
    assert.ok(input);
    input.value = "edited";
    const before = secondRenders;
    a.set([{ slot: "item", host: first, content: createElement(First, { text: " two" }) }]);
    assert.equal(first.textContent, "context two");
    assert.equal(secondRenders, before);
    assert.equal(second.querySelector("input"), input);
    assert.equal(input.value, "edited");
    assert.equal(firstRenders, 2);
    a.dispose();
    assert.equal(first.childElementCount, 0);
    assert.equal(second.querySelector("input"), input);
    a.set([{ slot: "item", host: first, content: createElement(First, { text: " late" }) }]);
    a.mount();
    flushSync(() => a.setDeferred([{ slot: "item", host: first, content: "late" }]));
    assert.equal(first.childElementCount, 0);
  } finally { a.dispose(); b.dispose(); first.remove(); second.remove(); }
});

test("document view renders synchronously inside its caller's flush and cannot revive after unmount", () => {
  const host = document.createElement("div");
  document.body.append(host);
  const view = createDocumentView(host);
  flushSync(() => view.render(createElement("input", { defaultValue: "draft" })));
  assert.equal(host.querySelector("input")?.value, "draft");
  view.unmount();
  flushSync(() => view.render(createElement("span", null, "late")));
  assert.equal(host.childElementCount, 0);
  host.remove();
});


test("document view replaces initial native contents once and retains its rendered controls", () => {
  const host = document.createElement("div"), placeholder = document.createElement("span");
  placeholder.textContent = "Loading";
  host.append(placeholder);
  document.body.append(host);
  const view = createDocumentView(host);
  assert.equal(host.firstChild, placeholder);
  flushSync(() => view.render(createElement("input", { defaultValue: "draft" })));
  const input = host.querySelector("input");
  assert.ok(input);
  assert.equal(placeholder.parentElement, null);
  input.value = "edited";
  flushSync(() => view.render(createElement("input", { defaultValue: "new" })));
  assert.equal(host.querySelector("input"), input);
  assert.equal(input.value, "edited");
  view.unmount();
  host.append(placeholder);
  flushSync(() => view.render(null));
  assert.equal(host.firstChild, placeholder);
  host.remove();
});
