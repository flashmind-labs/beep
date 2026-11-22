// The command center window as one React tree: the sidebar, the header, Home,
// the pane and the pages. Modules draw into named places of it and reach its
// elements through `windowHost`.
import { useLayoutEffect, type ReactNode } from "react";
import { createStore, useStore } from "zustand";
import { SIDEBAR, WidthGrip } from "../ui/width-grip.tsx";
import { HazeArt } from "../ui/painted-art.tsx";
import type { ViewName } from "./state.ts";

/// The places of the window that modules fill.
export type ShellPart = "sidebar" | "mobile-back" | "project-back" | "title" | "date" | "scope" | "tools" | "usage" | "notices"
  | "mobile-settings" | "home" | "home-composer" | "pages" | "mobile-tabs" | "palette" | "assistant" | "rail";

interface Shell {
  /// The view showing; null before the first.
  view: ViewName | null;
  /// The pane shows the Assistant page: its chats beside the bar, and whether a chat or the
  /// start composer is picked, which the phone shows in place of the chats.
  assistant: { picked: boolean } | null;
  parts: Partial<Record<ShellPart, ReactNode>>;
  side: Side;
}

/// What takes the sidebar's place: a project's sessions while one of them shows, or an
/// assistant chat's, by its running key, while it or one of its threads shows.
export type Rail = { kind: "project"; project: string } | { kind: "chat"; chat: string };

/// The sidebar: folded to its icons by the user, and the rail that takes its place, unless the
/// user unfolded the sidebar there.
interface Side { collapsed: boolean; rail: Rail | null; railHidden: boolean }

const sameRail = (a: Rail | null, b: Rail | null) =>
  a?.kind === "project" ? b?.kind === "project" && a.project === b.project : a?.kind === "chat" ? b?.kind === "chat" && a.chat === b.chat : !b;

/// The project whose sessions the rail shows, if any.
export const railProject = (rail: Rail | null) => rail?.kind === "project" ? rail.project : null;

export const shell = createStore<Shell>(() => ({ view: null, assistant: null, parts: { title: "Home" }, side: { collapsed: false, rail: null, railHidden: false } }));

/// Whether the sidebar shows only its icons, as the user left it. Beside a project's session the
/// sidebar and the project's sessions take turns in its full column, so the session stays put.
export const sideCollapsed = ({ collapsed, rail }: Side) => !rail && collapsed;

/// The sidebar's toggle: beside a rail it swaps the two, elsewhere it folds the sidebar.
export function toggleSide() {
  shell.setState(({ side }) => ({ side: side.rail ? { ...side, railHidden: !side.railHidden } : { ...side, collapsed: !side.collapsed } }));
}

/// Shows `rail` in place of the sidebar, or the sidebar again. Opening a session from elsewhere
/// shows its rail again.
export function setRail(rail: Rail | null) {
  shell.setState(now => sameRail(now.side.rail, rail) ? now : { side: { ...now.side, rail, railHidden: !!rail && !!now.side.rail && now.side.railHidden } });
}

/// Shows the sidebar again in place of the rail, until one of its sessions opens.
export function hideRail() {
  shell.setState(({ side }) => ({ side: { ...side, railHidden: true } }));
}

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
  const { view, assistant, parts, side } = useStore(shell);
  const collapsed = sideCollapsed(side), rail = !!side.rail && !side.railHidden;
  // The top bar's own width, which a session's title row keeps clear of (native.css).
  useLayoutEffect(() => {
    const top = hosts.get("top"), main = hosts.get("main");
    if (!top || !main) return;

    const measured = new ResizeObserver(() => main.style.setProperty("--top-w", `${top.offsetWidth}px`));
    measured.observe(top);
    return () => measured.disconnect();
  }, []);

  useLayoutEffect(() => {
    const win = hosts.get("side")?.parentElement;
    win?.classList.toggle("side-collapsed", collapsed);
    win?.classList.toggle("rail-on", rail);
  }, [collapsed, rail]);

  return <>
    <aside className="cc-side" id="side" ref={refs.side} inert={rail}><HazeArt />{parts.sidebar}</aside>
    <WidthGrip column={SIDEBAR} id="side-grip" />
    <aside className="cc-rail pj-list" id="rail" aria-label={side.rail?.kind === "chat" ? "Chat threads" : "Project sessions"} hidden={!side.rail} inert={!rail}>{side.rail && parts.rail}</aside>
    <div className="cc-main" id="main" ref={refs.main}>
      <div className="cc-top" data-tauri-drag-region="" ref={refs.top}>
        {parts["mobile-back"]}{parts["project-back"]}
        <h2 data-tauri-drag-region="" id="title">{parts.title}</h2>
        <span id="scope">{parts.scope}</span>
        <small id="date" data-tauri-drag-region="">{parts.date}</small>
        <span className="tools" id="tools">{parts.tools}</span>
        <span id="usage" className="us" hidden={parts.usage == null} ref={refs.usage}>{parts.usage}</span>
        <span id="bell" className="nt" ref={refs.bell}>{parts.notices}</span>
        {parts["mobile-settings"]}
      </div>
      <div className="cc-box" id="home-box" hidden={view !== null && view !== "home"} ref={refs.homeBox}>
        <div className="cc-view" id="view" ref={refs.view}>{parts.home}</div>
        {parts["home-composer"]}
      </div>
      {/* The Assistant page sets its chats beside the pane's bar, which `adoptBar` appends after them. */}
      <div className={`cc-box${assistant ? ` pj-split${assistant.picked ? " picked" : ""}` : ""}`} id="pane-box" hidden={view !== "pane"} ref={refs.paneBox}>{parts.assistant}</div>
      {parts.pages}
    </div>
    <nav className="mb-tabs" id="mb-tabs" aria-label="Sections">{parts["mobile-tabs"]}</nav>
    {parts.palette}
  </>;
}
