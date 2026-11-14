// The diff viewer's frame: a one-line header with the counts, a filter, icon
// toggles and the review buttons; a list of the files at wide widths, with a
// grip to resize it; the files themselves (diff-files.tsx); and the keys.
import { memo, useCallback, useEffect, useState, type CSSProperties } from "react";
import { useStore } from "zustand";
import { useShallow } from "zustand/react/shallow";
import { DiffToolbar } from "./diff-toolbar.tsx";
import { DiffTree } from "./diff-tree.tsx";
import { DiffKeys } from "./diff-status.tsx";
import { DiffFiles, fileMenu } from "./diff-files.tsx";
import { FeedbackBox, FeedbackNoteBox } from "./feedback-view.tsx";
import { allFolded, type DiffFile } from "./diff-state.ts";
import { NARROW, WIDE, type Viewer, keepCurrent, remeasure, remember, saveSplit, shown } from "./diff-viewer.ts";
import { foldAll, jump, jumpToLine, markCurrent, onKey } from "./diff-keys.ts";
import { load, sendReview } from "./diff.ts";
import { useTreeWidth } from "./treewidth.ts";

export function DiffViewer({ v }: { v: Viewer }) {
  const [body, setBody] = useState<HTMLDivElement | null>(null);
  const [tree, setTree] = useState<HTMLElement | null>(null);
  const [grip, setGrip] = useState<HTMLDivElement | null>(null);
  const width = useTreeWidth(body, grip);
  const bodyStyle: CSSProperties & Record<`--${string}`, string> = { "--dv-tree-width": `${width.width}px` };
  useRootEvents(v);

  return <>
    <div className="dv-head"><Toolbar v={v} /></div>
    {v.feedback ? <FeedbackNoteBox feedback={v.feedback} /> : <span />}
    <div className={`dv-body${width.resizing ? " resizing" : ""}`} ref={setBody} style={bodyStyle}>
      <nav className="dv-tree" aria-label="Changed files" ref={setTree}>{tree && <Tree v={v} scroller={tree} />}</nav>
      <div className="dv-resize" ref={setGrip} tabIndex={0} title="Drag to resize, double-click to reset" role="separator" aria-label="Resize changed files"
        aria-orientation="vertical" aria-valuemin={width.min} aria-valuemax={width.max} aria-valuenow={width.width}
        onDoubleClick={width.reset} onKeyDown={width.onKey} />
      <DiffFiles v={v} />
    </div>
    <div className="dv-keys"><DiffKeys /></div>
  </>;
}

/// The root is the caller's element: its keys, and its width, which picks the layout.
function useRootEvents(v: Viewer) {
  useEffect(() => {
    const root = v.root;
    const key = (event: KeyboardEvent) => onKey(v, event);
    root.addEventListener("keydown", key);
    const resize = new ResizeObserver(() => {
      root.classList.toggle("wide", root.clientWidth >= WIDE);
      const narrow = root.clientWidth <= NARROW;
      if (narrow === v.state.narrow) return;
      if (!v.state.split) { v.store.setState({ narrow }); return; }
      keepCurrent(v, () => { v.set({ narrow }); remeasure(v); });
    });
    resize.observe(root);
    return () => { root.removeEventListener("keydown", key); resize.disconnect(); };
  }, [v]);
}

const Toolbar = memo(function Toolbar({ v }: { v: Viewer }) {
  const s = useStore(v.store, useShallow(s => ({
    view: s.view, loading: s.loading, filter: s.filter, split: s.split, uncommitted: s.uncommitted, flash: s.flash,
    comments: s.draft.length, sending: s.sending, allFolded: allFolded(shown(s), s.folded),
  })));
  const findRef = useCallback((find: HTMLInputElement | null) => { v.find = find; }, [v]);
  return <DiffToolbar {...s} showUncommitted={!!v.src.uncommitted} readOnly={!!v.src.readOnly}
    feedback={v.feedback ? <FeedbackBox feedback={v.feedback} /> : <span />} findRef={findRef}
    onFilter={filter => {
      v.set({ filter, filteredClosedFolders: new Set() });
      remeasure(v);
      if (v.files) v.files.scrollTop = 0;
      markCurrent(v, false);
      remember(v);
    }}
    onSplit={() => {
      const split = !v.state.split;
      saveSplit(split);
      keepCurrent(v, () => { v.set({ split }); remeasure(v); });
    }}
    onUncommitted={() => { v.set({ uncommitted: !v.state.uncommitted }); void load(v); }}
    onFold={() => foldAll(v, !allFolded(shown(v.state), v.state.folded))} onRefresh={() => void load(v)} onSend={() => void sendReview(v)} />;
});

const Tree = memo(function Tree({ v, scroller }: { v: Viewer; scroller: HTMLElement }) {
  const files = useStore(v.store, s => s.view?.files ?? NO_FILES);
  const filter = useStore(v.store, s => s.filter), cur = useStore(v.store, s => s.cur), reveal = useStore(v.store, s => s.revealTree);
  const closed = useStore(v.store, s => s.closedFolders), filteredClosed = useStore(v.store, s => s.filteredClosedFolders);
  return <DiffTree files={files} filter={filter} current={cur} closedFolders={closed} filteredClosedFolders={filteredClosed}
    scroller={scroller} reveal={reveal}
    onFolder={(path, open) => {
      const filtering = !!v.state.filter.trim();
      const next = new Set(filtering ? v.state.filteredClosedFolders : v.state.closedFolders);
      if (open) next.add(path); else next.delete(path);
      v.set(filtering ? { filteredClosedFolders: next } : { closedFolders: next });
    }}
    onFile={path => jump(v, path)} onLine={(path, match) => jumpToLine(v, path, match)} onMenu={(event, file) => fileMenu(v, event.nativeEvent, file)} />;
});

const NO_FILES: readonly DiffFile[] = [];
