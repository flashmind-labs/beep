// Draws a list of elements into a container without touching the ones that did not change,
// so focus, a button mid-request, a "Copied" label and a spinner's animation stay where they are.
import { patchSteps, patchedSigs } from "./patch-state.ts";

interface Drawn {
  nodes: Element[];
  sigs: string[];
}

const drawn = new WeakMap<Element, Drawn>();

/// The indexes from `root` down to `at`, or null when `at` is not inside it.
function pathTo(root: Element, at: Element) {
  const path: number[] = [];
  for (let n: Element | null = at; n && n !== root; n = n.parentElement) {
    if (!n.parentElement) return null;
    path.unshift([...n.parentElement.children].indexOf(n));
  }
  return at === root || root.contains(at) ? path : null;
}

function follow(root: Element, path: number[]) {
  let n: Element | undefined = root;
  for (const i of path) n = n?.children[i];
  return n;
}

/// Makes `container` hold `next`. A child whose markup did not change stays as it is, one with a
/// request in flight (a disabled button) stays until that answers, and a changed one is replaced
/// with its focus moved to the same place in the new one.
export function patchChildren(container: Element, next: Element[]) {
  const was = drawn.get(container);
  const sigs = next.map(n => n.outerHTML);
  const intact = was && was.nodes.length === container.children.length && was.nodes.every((n, i) => container.children[i] === n);
  if (!was || !intact) {
    container.replaceChildren(...next);
    drawn.set(container, { nodes: [...next], sigs });
    return;
  }
  const steps = patchSteps(was.sigs, sigs, i => !!was.nodes[i].querySelector("button:disabled"));
  const focus = document.activeElement;
  const nodes: Element[] = [];
  steps.forEach((step, i) => {
    const old = was.nodes[i];
    if (step === "keep") return nodes.push(old);
    if (step === "add") { container.append(next[i]); return nodes.push(next[i]); }
    const path = focus && old.contains(focus) ? pathTo(old, focus) : null;
    old.replaceWith(next[i]);
    nodes.push(next[i]);
    const to = path && follow(next[i], path);
    if (to instanceof HTMLElement && to.tagName === (focus as Element).tagName) to.focus({ preventScroll: true });
  });
  for (const gone of was.nodes.slice(next.length)) gone.remove();
  drawn.set(container, { nodes, sigs: patchedSigs(was.sigs, sigs, steps) });
}
