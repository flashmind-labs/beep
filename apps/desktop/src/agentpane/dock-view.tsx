// The session's shell and the pane's docks, drawn from the model in
// media-layout.ts: each dock's divider, tab strip, controls and views, its
// strip while folded, the drop zone and ghost of a tab drag, and the phone's
// tab strip.
import { memo, useLayoutEffect, useRef, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { useStore } from "zustand";
import { Icon } from "../ui/icons.tsx";
import { dragHandle } from "../ui/drag.ts";
import type { Dock } from "./media-state.ts";
import { type DockModel, type DockView, dividerDrag, dividerKey, mainRef, paneModel, paneRefs, resetDivider, shellRef, stripRefs } from "./media-layout.ts";
import { foldDock, moveDock, pickPhoneTab, pickTab, tabMenu } from "./media-draw.ts";
import { closeTab, newTerminal, P, type MediaTab } from "./media.ts";
import { ghostRef, tabDown } from "./drag.ts";
import { PaneTab, PaneTabs, PhonePaneTabs, type PaneTabActions } from "./tabs-view.tsx";
import { WorktreeMenu } from "./worktree-view.tsx";
import { FilesView } from "./files-tree.tsx";
import { ArtifactsView } from "./artifacts-list.tsx";
import { RunningPane } from "./running-view.tsx";
import { DevicePane } from "./device-view.tsx";
import { SubagentsView } from "./subagents-view.tsx";
import { changesModel, leftChanges, placedChanges } from "./changes.ts";
import { TextView } from "./text-view.tsx";
import { ImageView } from "./image-view.tsx";
import { ModelView, PdfView, PendingView, VideoView } from "./media-view.tsx";
import { TermView } from "./term-footer.tsx";

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

/// Dock `dock`: its divider, the pane with its head and views, and its strip while folded.
export function PaneDock({ dock }: { dock: Dock }) {
  const model = useStore(paneModel, s => s.docks?.[dock] ?? null);
  const resizing = useStore(paneModel, s => s.resizing === dock);
  const dragging = useStore(paneModel, s => s.drag?.item.id ?? null);
  const divider = useRef<HTMLDivElement>(null);
  // The divider says its orientation once its dock showed.
  const oriented = useRef(false);
  if (model?.shown) oriented.current = true;

  useLayoutEffect(() => {
    const div = divider.current;
    if (div) dragHandle(div, () => dividerDrag(dock));
  }, [dock]);

  const bottom = dock === "bottom";
  const hidden = !model?.shown;
  return <><div ref={divider} className={`agdiv${resizing ? " active" : ""}`} id={`agdiv-${dock}`} data-dock={dock} role="separator" aria-orientation={oriented.current ? bottom ? "horizontal" : "vertical" : undefined} tabIndex={0} title="Drag to resize, double-click to reset" hidden={hidden}
      onDoubleClick={() => resetDivider(dock)} onKeyDown={event => { if (dividerKey(dock, event.key)) { event.preventDefault(); event.stopPropagation(); } }} />
    <aside ref={paneRefs[dock]} className={`agpane${model?.zoomed ? " zoomed" : ""}`} id={`agpane-${dock}`} data-dock={dock} tabIndex={-1} hidden={hidden} style={model?.size != null ? { flexBasis: `var(--drag-size, ${model.size}px)` } : undefined}
      onPointerDownCapture={event => { const pane = event.currentTarget; if (!pane.contains(document.activeElement)) pane.focus({ preventScroll: true }); }}>
      <div className="agpane-content"><div className="agphead">
        <div className="agtabs" role="tablist">{model && <PaneTabs tabs={model.tabs} selected={model.on} dragging={dragging} actions={ACTIONS[dock]} />}</div>
        <span className="agwtw" hidden={!model?.worktrees}>{model?.worktrees && <WorktreeMenu {...model.worktrees} />}</span>
        <button className="agpbtn agaddterm" data-pact="term" title="New terminal in this folder (⌃`)" onClick={() => newTerminal(dock)}>+ Terminal</button>
        <button className="agpbtn" data-pact="dock" title={model ? bottom ? "Dock these tabs on the right" : "Dock these tabs at the bottom" : undefined} onClick={() => moveDock(dock)}>{model && <span style={{ display: "contents" }}>{bottom ? DOCK_RIGHT : DOCK_BOTTOM}</span>}</button>
        <button className="agpbtn" data-pact="fold" title="Hide the pane" onClick={() => foldDock(dock, true)}>{model && <span style={{ display: "contents" }}>{bottom ? FOLD_DOWN : FOLD_RIGHT}</span>}</button>
      </div><div className="agpviews">{model?.views.map(view => <PaneView key={view.kind === "tab" ? view.tab.id : view.kind} view={view} />)}</div></div>
    </aside>
    <button ref={stripRefs[dock]} className="agpstrip" id={`agpstrip-${dock}`} data-dock={dock} hidden={!model?.strip} title="Show the pane" onClick={() => foldDock(dock, false)}>{model && <DockStrip model={model} />}</button></>;
}

function DockStrip({ model }: { model: DockModel }) {
  return <><span style={{ display: "contents" }}><Icon name={model.icon} /></span><span>{model.label}</span><small>{model.tabs.length}</small></>;
}

function PaneView({ view }: { view: DockView }) {
  switch (view.kind) {
    case "files": return <FilesView hidden={view.hidden} />;
    case "changes": return <ChangesView hidden={view.hidden} />;
    case "artifacts": return <ArtifactsView hidden={view.hidden} />;
    case "running": return <RunningPane hidden={view.hidden} />;
    case "device": return <DevicePane hidden={view.hidden} />;
    case "subagents": return <div className="agpview subs" hidden={view.hidden}><SubagentsView rows={P.subs} /></div>;
    case "tab": return <TabView tab={view.tab} rev={view.rev} hidden={view.hidden} />;
  }
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
/// a PDF or a 3D model, or its load while it reads.
const TabView = memo(function TabView({ tab, rev, hidden }: { tab: MediaTab; rev: number; hidden: boolean }) {
  const machine = P.session?.machine_id ?? null;
  if (tab.kind === "term" && tab.term && !tab.error) return <TermView tab={tab} term={tab.term} hidden={hidden} rev={rev} />;
  const ready = !tab.error && (tab.kind === "text" ? !!tab.text : tab.kind === "term" ? false : tab.kind === "model" ? !!tab.bytes : !!tab.url);
  return <div className="agpview" hidden={hidden}>{!ready ? <PendingView tab={tab} />
    : tab.kind === "text" && tab.text ? <TextView tab={tab} text={tab.text} rev={rev} />
    : tab.kind === "image" ? <ImageView tab={tab} machine={machine} />
    : tab.kind === "video" ? <VideoView tab={tab} machine={machine} />
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
