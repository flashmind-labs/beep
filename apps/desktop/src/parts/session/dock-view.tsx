// The session's shell and the pane's docks, drawn from the model in
// media-layout.ts: each dock's divider, tab strip, controls and views, its
// strip while folded, the drop zone and ghost of a tab drag, and the phone's
// tab strip.
import { memo, useLayoutEffect, useRef, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { useStore } from "zustand";
import { onPhone, primaryModifier } from "../../ui/platform.ts";
import { dragHandle } from "../../ui/drag.ts";
import type { Dock } from "../../stores/agentpane/media-state.ts";
import { type DockView, mainRef, openTabs, paneModel, paneRefs, shellRef, stripRefs } from "./media-layout.ts";
import { foldDock, moveDock, pickTab, tabMenu } from "./dock-actions.ts";
import { dockModel, pickPhoneTab } from "./media-draw.ts";
import { newTerminal } from "./media.ts";
import { dividerDrag, dividerKey, resetDivider } from "./pane-input.ts";
import { P, type MediaTab } from "../../stores/agentpane/pane-state.ts";
import { closeTab, touchTab } from "./pane-tabs.ts";
import { ghostRef, tabDown } from "./drag.ts";
import { PaneTab, PaneTabs, PhonePaneTabs, type PaneTabActions } from "./tabs-view.tsx";
import { WorktreeMenu } from "./worktree-view.tsx";
import { FilesView } from "./files-tree.tsx";
import { ArtifactsView } from "./artifacts-list.tsx";
import { RunningPane } from "./running-view.tsx";
import { DevicePane } from "./device-view.tsx";
import { SubagentsView } from "./subagents-view.tsx";
import { ChatAgentsPane } from "./chat-agents.tsx";
import { changesModel, leftChanges, placedChanges } from "./changes.ts";
import { TextView } from "./text-view.tsx";
import { ImageView } from "./image-view.tsx";
import { ModelView, PdfView, PendingView, PlayerView } from "./media-view.tsx";
import { TermView } from "./term-footer.tsx";
import { SessionTabView, type SessionTabLinks } from "./session-tab-view.tsx";
import { replyToQuestion, sendOnce } from "./messages.ts";
import { feed, keptTranscript } from "./session-follow.ts";

/// The session's shell: the conversation with the bottom dock under it, and the right dock.
export function PaneShell({ children }: { children: ReactNode }) {
  const { narrow, zoomed, resizing, drag, phone, log } = useStore(paneModel);
  const classes = ["agshell", narrow && "narrow", zoomed && "zoomed", resizing && `resizing resizing-${resizing}`, drag && "tabdrag", phone && log && "mb-log"];
  return <div className={classes.filter(Boolean).join(" ")} id="agshell" ref={shellRef}>{children}</div>;
}

/// The conversation and the bottom dock under it, led on a phone by its tab strip.
export function PaneMain({ children }: { children: ReactNode }) {
  const phone = useStore(paneModel, s => s.phone);
  return <div className="agmain" id="agmain" ref={mainRef}>{phone && <nav id="agmnav" className="agmnav"><PhonePaneTabs tabs={phone.tabs} selected={phone.selected} select={pickPhoneTab} /></nav>}{children}</div>;
}

const DOCK_RIGHT = <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><rect x="3" y="4" width="18" height="16" rx="2" /><path d="M15 4v16" /></svg>;
const DOCK_BOTTOM = <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><rect x="3" y="4" width="18" height="16" rx="2" /><path d="M3 14h18" /></svg>;
const FOLD_RIGHT = <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round"><path d="M9 6l6 6-6 6" /></svg>;
const FOLD_DOWN = <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round"><path d="M6 9l6 6 6-6" /></svg>;

const tabActions = (d: Dock): PaneTabActions => ({
  pick: id => pickTab(d, id),
  close: closeTab,
  menu: (event, id) => tabMenu(event.nativeEvent, id),
  down: (event, id) => tabDown(event.nativeEvent, id, event.currentTarget),
});
const ACTIONS: Record<Dock, PaneTabActions> = { right: tabActions("right"), bottom: tabActions("bottom") };
/// A folded dock keeps its tabs in its strip: picking one opens the dock on it.
const unfold = (d: Dock): PaneTabActions => ({ ...ACTIONS[d], pick: id => { foldDock(d, false); pickTab(d, id); } });
const UNFOLD: Record<Dock, PaneTabActions> = { right: unfold("right"), bottom: unfold("bottom") };

// Read when drawn, as these modules reach this one and may not have run when it loads.
const sessionLinks = (): SessionTabLinks => ({ feed, keptTranscript, sendOnce, replyToQuestion });

/// Dock `dock`: its divider, the pane with its head and views, and its strip while folded.
export function PaneDock({ dock }: { dock: Dock }) {
  const model = useStore(paneModel, s => dockModel(s, dock));
  const { shown, strip, size } = useStore(paneModel, s => s.places[dock]);
  const on = useStore(paneModel, s => s.on[dock]);
  const zoomed = useStore(paneModel, s => s.zoomed === dock);
  const resizing = useStore(paneModel, s => s.resizing === dock);
  const dragging = useStore(paneModel, s => s.drag?.item.id ?? null);
  const divider = useRef<HTMLDivElement>(null);
  // The divider says its orientation once its dock showed.
  const oriented = useRef(false);
  if (shown) oriented.current = true;

  useLayoutEffect(() => {
    const div = divider.current;
    if (div) dragHandle(div, () => dividerDrag(dock));
  }, [dock]);

  const bottom = dock === "bottom";
  const hidden = !shown;
  return <><div ref={divider} className={`agdiv${resizing ? " active" : ""}`} id={`agdiv-${dock}`} data-dock={dock} role="separator" aria-orientation={oriented.current ? bottom ? "horizontal" : "vertical" : undefined} tabIndex={0} title="Drag to resize, double-click to reset" hidden={hidden}
      onDoubleClick={() => resetDivider(dock)} onKeyDown={event => { if (dividerKey(dock, event.key)) { event.preventDefault(); event.stopPropagation(); } }} />
    <aside ref={paneRefs[dock]} className={`agpane${zoomed ? " zoomed" : ""}`} id={`agpane-${dock}`} data-dock={dock} tabIndex={-1} hidden={hidden} style={size != null ? { flexBasis: `var(--drag-size, ${size}px)` } : undefined}
      onKeyDownCapture={event => {
        if (event.key.toLowerCase() !== "f" || event.nativeEvent.isComposing || !primaryModifier(event)) return;
        const tab = openTabs().find(tab => tab.id === on);
        if (!tab?.text || tab.text.binary || (!tab.edit && tab.preview?.active) || onPhone()) return;

        event.preventDefault();
        event.stopPropagation();
        tab.findRequest = (tab.findRequest ?? 0) + 1;
        touchTab(tab);
      }}
      onPointerDownCapture={event => { const pane = event.currentTarget; if (!pane.contains(document.activeElement)) pane.focus({ preventScroll: true }); }}>
      <div className="agpane-content"><div className="agphead">
        <div className="agtabs" role="tablist">{model && <PaneTabs tabs={model.tabs} selected={on} dragging={dragging} actions={ACTIONS[dock]} />}</div>
        <span className="agwtw" hidden={!model?.worktrees}>{model?.worktrees && <WorktreeMenu {...model.worktrees} />}</span>
        <button className="agpbtn agaddterm" data-pact="term" title="New terminal in this folder (⌃`)" onClick={() => newTerminal(dock)}>+ Terminal</button>
        <button className="agpbtn" data-pact="dock" title={model ? bottom ? "Dock these tabs on the right" : "Dock these tabs at the bottom" : undefined} onClick={() => moveDock(dock)}>{model && <span style={{ display: "contents" }}>{bottom ? DOCK_RIGHT : DOCK_BOTTOM}</span>}</button>
        <button className="agpbtn" data-pact="fold" title="Hide the pane" onClick={() => foldDock(dock, true)}>{model && <span style={{ display: "contents" }}>{bottom ? FOLD_DOWN : FOLD_RIGHT}</span>}</button>
      </div><div className="agpviews">{model?.views.map(view => <PaneView key={view.kind === "tab" ? view.tab.id : view.kind} view={view} />)}</div></div>
    </aside>
    <div ref={el => { stripRefs[dock].current = el; }} className="agpstrip tabs" id={`agpstrip-${dock}`} data-dock={dock} role="tablist" aria-orientation={bottom ? "horizontal" : "vertical"} hidden={!strip}>{strip && model && <PaneTabs tabs={model.tabs} selected={on} dragging={dragging} actions={UNFOLD[dock]} />}</div></>;
}

function PaneView({ view }: { view: DockView }) {
  switch (view.kind) {
    case "files": return <FilesView hidden={view.hidden} />;
    case "changes": return <ChangesView hidden={view.hidden} />;
    case "artifacts": return <ArtifactsView hidden={view.hidden} />;
    case "running": return <RunningPane hidden={view.hidden} />;
    case "device": return <DevicePane hidden={view.hidden} />;
    case "agents": return <ChatAgentsPane hidden={view.hidden} />;
    case "subagents": return <SubagentsPane hidden={view.hidden} />;
    case "tab": return <TabView tab={view.tab} rev={view.rev} hidden={view.hidden} />;
  }
}

function SubagentsPane({ hidden }: { hidden: boolean }) {
  const rows = useStore(paneModel, s => s.subs);
  return <div className="agpview subs" hidden={hidden}><SubagentsView rows={rows} /></div>;
}

/// The Changes tab: the diff viewer's own element, kept per session, goes into the view.
const ChangesView = memo(function ChangesView({ hidden }: { hidden: boolean }) {
  const { entry, note } = useStore(changesModel);
  const host = useRef<HTMLDivElement>(null);

  useLayoutEffect(() => {
    const el = host.current;
    if (!el || !entry) return;
    el.append(entry.view.el);
    placedChanges(entry);
    return () => leftChanges(entry);
  }, [entry]);

  return <div className="agpview changes" hidden={hidden} ref={host}>{entry && <div className="dv-note" hidden={!note}>{note}</div>}</div>;
});

/// What an opened tab shows: a file as text, a terminal, a picture, a video,
/// audio, a PDF or a 3D model, or its load while it reads.
/// `rev` is only a memo key: it draws again when the tab changed.
const TabView = memo(function TabView({ tab, hidden }: { tab: MediaTab; rev: number; hidden: boolean }) {
  const machine = P.session?.machine_id ?? null;
  if (tab.kind === "term" && tab.term && !tab.error) return <TermView tab={tab} term={tab.term} hidden={hidden} />;
  if (tab.kind === "session" && tab.splitSession) return <div className="agpview agsplit-view" hidden={hidden}><SessionTabView session={tab.splitSession} hidden={hidden} close={() => closeTab(tab.id)} links={sessionLinks()} /></div>;
  const ready = !tab.error && (tab.kind === "text" ? !!tab.text : tab.kind === "term" ? false : tab.kind === "model" ? !!tab.bytes : !!(tab.url || tab.exported));
  return <div className="agpview" hidden={hidden}>{!ready ? <PendingView tab={tab} />
    : tab.kind === "text" && tab.text ? <TextView tab={tab} text={tab.text} />
    : tab.kind === "image" ? <ImageView tab={tab} machine={machine} />
    : tab.kind === "video" ? <PlayerView tab={tab} machine={machine} video />
    : tab.kind === "audio" ? <PlayerView tab={tab} machine={machine} video={false} />
    : tab.kind === "pdf" ? <PdfView tab={tab} machine={machine} />
    : tab.bytes ? <ModelView tab={tab} bytes={tab.bytes} machine={machine} /> : <PendingView tab={tab} />}</div>;
});

/// Where a dragged tab would go, and its ghost under the pointer.
export function PaneDropZone() {
  const drag = useStore(paneModel, s => s.drag);
  const zone = drag?.zone;
  return <><div className="agdropzone" id="agdropzone" hidden={!drag?.to} style={zone ? { left: `${zone.left}px`, top: `${zone.top}px`, width: `${zone.width}px`, height: `${zone.height}px` } : undefined}>
    {drag?.to && <><span style={{ display: "contents" }}>{drag.to === "right" ? DOCK_RIGHT : DOCK_BOTTOM}</span><span>{drag.to === "right" ? "Dock on the right" : "Dock at the bottom"}</span></>}</div>
    {drag && createPortal(<PaneTab ref={ghostRef} tab={drag.item} className={`agtab agtabghost${drag.selected ? " on" : ""}`} selected={drag.selected} />, document.body)}</>;
}
