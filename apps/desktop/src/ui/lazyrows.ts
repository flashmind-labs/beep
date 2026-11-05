// Long lists drawn a screenful at a time: the first rows now, and a spacer
// after them that draws the next ones once it nears the viewport. Rows are
// only ever added below, so scrolling up never jumps.

/// Rows drawn per step, unless a list asks for another.
const CHUNK = 40;
/// How far ahead of the scrolled area the next rows are drawn.
const AHEAD = "800px 0px";

/// What one list has drawn so far.
interface Drawn {
  /// What the list shows (a search, a filter); a new one starts over.
  token: string;
  count: number;
  /// A row's height once measured, so a fresh spacer is as tall as the rows it stands in for.
  height: number;
}

const lists = new Map<string, Drawn>();
const steps = new WeakMap<Element, (near: boolean) => void>();
/// One observer per scrolling parent, since a margin only reaches past the edge of its own root.
const watchers = new Map<Element | null, IntersectionObserver>();

/// Starts the lists whose key begins with `prefix` over, as when their page opens again.
export function resetRows(prefix: string) {
  for (const key of [...lists.keys()]) if (key.startsWith(prefix)) lists.delete(key);
}

function scrollParent(el: Element) {
  for (let p = el.parentElement; p; p = p.parentElement) {
    if (/(auto|scroll)/.test(getComputedStyle(p).overflowY)) return p;
  }
  return null;
}

function watch(end: Element) {
  const root = scrollParent(end);
  let observer = watchers.get(root);
  if (!observer) {
    observer = new IntersectionObserver(entries => {
      for (const e of entries) steps.get(e.target)?.(e.isIntersecting);
    }, { root, rootMargin: AHEAD });
    watchers.set(root, observer);
  }
  return observer;
}

function unwatch(end: Element) {
  for (const observer of watchers.values()) observer.unobserve(end);
}

/// Rows `0..total` of list `key` as nodes: the first screenful, then a spacer
/// that draws more when it nears the viewport. `make(from, to)` builds those
/// rows. `need` is a row that must be drawn now, such as the selected one.
export function lazyRows(key: string, token: string, total: number, make: (from: number, to: number) => Node[], need = 0, chunk = CHUNK): Node[] {
  if (typeof IntersectionObserver === "undefined") return make(0, total);
  let list = lists.get(key);
  if (!list || list.token !== token) lists.set(key, list = { token, count: chunk, height: 0 });
  const drawn = list;
  drawn.count = Math.min(total, Math.max(drawn.count, need + 1));
  const rows = make(0, drawn.count);
  if (drawn.count >= total) return rows;
  const end = document.createElement("div");
  // Takes a whole row of a grid, not one cell.
  end.style.gridColumn = "1 / -1";
  if (drawn.height) end.style.height = `${(total - drawn.count) * drawn.height}px`;
  const first = rows[0] as Element | undefined;
  steps.set(end, near => {
    if (!end.isConnected || lists.get(key) !== drawn) return;
    if (!drawn.height && first) {
      // Zero while the page is hidden; measured again on the next step.
      drawn.height = Math.max(0, (end.getBoundingClientRect().top - first.getBoundingClientRect().top) / drawn.count);
      end.style.height = `${(total - drawn.count) * drawn.height}px`;
    }
    if (!near) return;
    const from = drawn.count;
    drawn.count = Math.min(total, from + chunk);
    end.before(...make(from, drawn.count));
    unwatch(end);
    if (drawn.count >= total) return end.remove();
    end.style.height = `${(total - drawn.count) * drawn.height}px`;
    // Watched again, so a spacer still near the viewport draws the next rows too.
    watch(end).observe(end);
  });
  // The spacer is on the page by the next frame, which is when its scrolling parent is known.
  requestAnimationFrame(() => { if (end.isConnected) watch(end).observe(end); });
  return [...rows, end];
}
