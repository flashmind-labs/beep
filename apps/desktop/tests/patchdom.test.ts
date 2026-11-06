import "./dom-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { patchChildren } from "../src/ui/patch.ts";

function el(html: string) {
  const box = document.createElement("div");
  box.innerHTML = html;
  return box.firstElementChild!;
}

test("a child drawn with a disabled button is replaced by the next draw (#330)", () => {
  const view = document.createElement("div");
  // The Machines tab: an offline machine's New terminal is drawn disabled.
  patchChildren(view, [el(`<nav>tabs</nav>`), el(`<section id="sec-machines"><button disabled>New terminal</button></section>`)]);
  // Overview opens in its place.
  patchChildren(view, [el(`<nav>tabs</nav>`), el(`<section id="sec-needs">Needs you</section>`)]);
  assert.equal(view.children[1].id, "sec-needs");
});

test("a button disabled after drawing, a request in flight, keeps its child", () => {
  const view = document.createElement("div");
  patchChildren(view, [el(`<section id="a"><button>Allow</button></section>`)]);
  const busy = view.children[0];
  (busy.querySelector("button") as HTMLButtonElement).disabled = true;
  patchChildren(view, [el(`<section id="b">Next</section>`)]);
  assert.equal(view.children[0], busy);
});
