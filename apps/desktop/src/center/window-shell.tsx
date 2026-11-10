// The command center window as one React tree: the sidebar, the header, Home,
// the pane and the pages. Modules draw into named places of it and reach its
// elements through `windowHost`.
import type { ReactNode } from "react";
import { createStore, useStore } from "zustand";
import { SideGrip } from "./sidewidth.tsx";
import type { ViewName } from "./state.ts";

/// The places of the window that modules fill.
export type ShellPart = "sidebar" | "mobile-back" | "project-back" | "title" | "date" | "scope" | "tools" | "usage" | "notices"
  | "home" | "home-composer" | "pages" | "mobile-tabs" | "palette";

interface Shell {
  /// The view showing; null before the first.
  view: ViewName | null;
  parts: Partial<Record<ShellPart, ReactNode>>;
}

export const shell = createStore<Shell>(() => ({ view: null, parts: { title: "Home" } }));

/// The window's elements other modules work in.
type Host = "side" | "main" | "top" | "home-box" | "view" | "pane-box" | "usage" | "bell";
const hosts = new Map<Host, HTMLElement>();
const keep = (name: Host) => (el: HTMLElement | null) => { if (el) hosts.set(name, el); else hosts.delete(name); };
const refs = {
  side: keep("side"), main: keep("main"), top: keep("top"), homeBox: keep("home-box"), view: keep("view"),
  paneBox: (el: HTMLElement | null) => { keep("pane-box")(el); adoptBar(el); }, usage: keep("usage"), bell: keep("bell"),
};

/// Moves the page's static `#bar` into the pane box. The bar's modules hold it from
/// before the window renders, so it stays the same element.
function adoptBar(box: HTMLElement | null) {
  const bar = document.getElementById("bar");
  if (box && bar && bar.parentElement !== box) box.append(bar);
}

/// Element `name` of the window. The window renders before any module asks.
export function windowHost(name: Host): HTMLElement {
  const host = hosts.get(name);
  if (!host) throw new Error(`The window has no ${name}`);
  return host;
}

export function CenterWindow() {
  const { view, parts } = useStore(shell);
  return <>
    <aside className="cc-side" id="side" ref={refs.side}>{parts.sidebar}</aside>
    <SideGrip />
    <div className="cc-main" id="main" ref={refs.main}>
      <div className="cc-top" data-tauri-drag-region="" ref={refs.top}>
        {parts["mobile-back"]}{parts["project-back"]}
        <h2 data-tauri-drag-region="" id="title">{parts.title}</h2>
        <span id="scope">{parts.scope}</span>
        <small id="date" data-tauri-drag-region="">{parts.date}</small>
        <span className="tools" id="tools">{parts.tools}</span>
        <span id="usage" className="us" hidden={parts.usage == null} ref={refs.usage}>{parts.usage}</span>
        <span id="bell" className="nt" ref={refs.bell}>{parts.notices}</span>
      </div>
      <div className="cc-box" id="home-box" hidden={view !== null && view !== "home"} ref={refs.homeBox}>
        <div className="cc-view" id="view" ref={refs.view}>{parts.home}</div>
        {parts["home-composer"]}
      </div>
      <div className="cc-box" id="pane-box" hidden={view !== "pane"} ref={refs.paneBox} />
      {parts.pages}
    </div>
    <nav className="mb-tabs" id="mb-tabs" aria-label="Sections">{parts["mobile-tabs"]}</nav>
    {parts.palette}
  </>;
}
