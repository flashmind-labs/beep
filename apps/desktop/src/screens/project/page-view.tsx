import { Fragment, useLayoutEffect, useRef, type ReactElement } from "react";
import { Icon } from "../../ui/icons.tsx";
import { Spinner } from "../../ui/kit.tsx";
import { PaintedEmpty } from "../../ui/painted-art.tsx";
import type { Freshness } from "../../ui/pagecache.ts";

/// One child of a Projects page container: an element React renders, or a
/// host its feature owns. A view renders exactly one element.
export type PagePart = { kind: "view"; key: string; view: ReactElement } | { kind: "host"; host: HTMLElement };

export const hostPart = (host: HTMLElement): PagePart => ({ kind: "host", host });
export const viewPart = (key: string, view: ReactElement): PagePart => ({ kind: "view", key, view });

/// The element of a host part. React renders into it through a portal and `Parts` only
/// places it, so its rows keep scroll and focus while the page around it changes or the
/// view is away. Without a class it takes no box of its own.
export function retainedHost(className: string | null, { tag = "div", label = null }: { tag?: "div" | "aside"; label?: string | null } = {}) {
  const host = document.createElement(tag);
  if (className === null) host.style.display = "contents";
  else host.className = className;
  if (label !== null) host.setAttribute("aria-label", label);
  return host;
}

/// A container holding views and retained hosts as direct children. A host
/// moves only when its place changes, so a scroll, focus, selection or a
/// click inside it survives every draw.
export function Parts({ className, project, parts }: { className: string; project?: string; parts: readonly PagePart[] }) {
  const box = useRef<HTMLDivElement>(null);
  const placed = useRef<readonly HTMLElement[]>([]);

  useLayoutEffect(() => {
    const target = box.current;
    if (target) placed.current = placeParts(target, parts, placed.current);
  });

  return <div ref={box} className={className} data-project={project}>
    {parts.map(part => part.kind === "view" && <Fragment key={part.key}>{part.view}</Fragment>)}
  </div>;
}

/// Puts `parts` in order in `box`. React keeps its own elements in order, so
/// they are the children that are not hosts; only nodes out of place move.
function placeParts(box: HTMLElement, parts: readonly PagePart[], before: readonly HTMLElement[]) {
  const hosts = parts.flatMap(part => part.kind === "host" ? [part.host] : []);
  const wanted = new Set<Element>(hosts);
  for (const old of before) if (!wanted.has(old) && old.parentElement === box) old.remove();

  const own = [...box.children].filter(child => !wanted.has(child));
  let next = 0;
  const order = parts.flatMap(part => part.kind === "host" ? [part.host] : own.slice(next, ++next));

  order.forEach((node, i) => {
    const there = box.children[i];
    if (there !== node) box.insertBefore(node, there ?? null);
  });

  return hosts;
}

export function NoProjects({ add }: { add: () => void }) {
  return <PaintedEmpty>
    <b>No projects yet</b>
    <span>Choose a folder, clone a repository or start a new one.</span>
    <button type="button" className="pri" onClick={add}><Icon name="plus"/> Add a project</button>
  </PaintedEmpty>;
}

/// The note on a list being read again: a still dot, else how old it is.
export function FreshNote({ freshness, className = "" }: { freshness: Freshness; className?: string }) {
  if (freshness.spin) return <Spinner dot className={`sw-spin${className}`} title="Updating" />;
  return freshness.text ? <small className={`sw-age${className}`}>{freshness.text}</small> : null;
}
