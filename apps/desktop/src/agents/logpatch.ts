// Puts a redrawn log into the page without replacing what did not change, so
// a poll that brings one new line costs one row, not the whole log. A row
// kept is the same element: its selection, its scroll, its animation.

import { reuseRows } from "./logdiff-state.ts";
import { setUnits, type Unit } from "./logwindow.ts";

/// The html each child of a container was drawn from.
const drawnFrom = new WeakMap<Element, string[]>();

/// Stands in for the log, which is patched by its turns.
const NESTED = "\0log";
const isLog = (el: Element) => el.classList.contains("dcoderlog");

/// Whether `main` was drawn by `patchRows`, and so can be patched.
export const patchable = (main: Element | null) => !!main && drawnFrom.has(main);

/// Marks `main` as empty, ready to be patched.
export const startPatching = (main: Element) => drawnFrom.set(main, []);

/// Makes the children of `live` those of `next`, whose `.dcoderlog` is
/// empty: that one holds `units`, the log's turns, instead (logwindow.ts).
/// A child whose html is the one it was drawn from stays; the others are
/// drawn new. Returns false, changing nothing at this level, when a child
/// that would go holds one of `held`, the nodes a selection starts or ends in.
export function patchRows(live: Element, next: ParentNode, units: readonly Unit[] | null, held: readonly Node[] = []): boolean {
  const kids = [...live.children], fresh = [...next.children];
  const was = [...(drawnFrom.get(live) ?? [])];
  const html = fresh.map(el => el.outerHTML);
  const nestedKid = kids.findIndex(isLog), nestedFresh = fresh.findIndex(isLog);
  if (units && nestedFresh >= 0) {
    if (!setUnits((nestedKid >= 0 ? kids[nestedKid] : fresh[nestedFresh]) as HTMLElement, units, held)) return false;
    if (nestedKid >= 0) was[nestedKid] = NESTED;
    html[nestedFresh] = NESTED;
  }
  const reuse = reuseRows(was, html);
  const keep = new Set(reuse.filter(i => i >= 0));
  const going = kids.filter((_, i) => !keep.has(i));
  if (going.some(el => held.some(n => el.contains(n)))) return false;
  for (const el of going) el.remove();
  let cursor: Element | null = live.firstElementChild;
  fresh.forEach((el, i) => {
    const now = reuse[i] >= 0 ? kids[reuse[i]] : el;
    if (now === cursor) cursor = cursor.nextElementSibling;
    else live.insertBefore(now, cursor);
  });
  drawnFrom.set(live, html);
  return true;
}
