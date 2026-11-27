import { useCallback, useLayoutEffect, useMemo, useRef, useState } from "react";
import { defaultRangeExtractor, useVirtualizer, type Range } from "@tanstack/react-virtual";
import { base, formatAgo } from "../../ui/format.ts";
import { HarnessIcon } from "../../ui/icons.tsx";
import { placeOf } from "../../stores/agents/place.ts";
import { placeSub } from "../../stores/agents/place-state.ts";
import { agentLabel, type SessionRow } from "../../stores/project/projects-state.ts";
import { sessionMenu } from "../../parts/session/sessionmenu.ts";
import { go } from "../../parts/center/nav.ts";

/// A project's past sessions: five rows tall in a scroll box, or `all` of
/// them, virtual once there are many. Each opens its agent.
export function ProjectSessions({ rows, all }: { rows: SessionRow[]; all: boolean }) {
  const open = (row: SessionRow) => go("pane", { agent: { kind: "agent", ...row } });
  const menu = (event: MouseEvent, row: SessionRow) => void sessionMenu(event, row, selected => go("pane", { agent: selected }));

  // Keep the selected rendering mode for this mounted list so a refresh cannot
  // replace a focused row when the list crosses the threshold.
  const [virtual] = useState(() => all && rows.length > 100);
  return virtual ? <VirtualSessions rows={rows} open={open} menu={menu}/> : <SessionList rows={rows} all={all} open={open} menu={menu}/>;
}

type SessionProps = { rows: SessionRow[]; open: (row: SessionRow) => void; menu: (event: MouseEvent, row: SessionRow) => void };
const sessionKey = (row: SessionRow) => `${row.machine_id}:${row.agent}:${row.id}`;

function SessionRowView({ row: s, open, menu, index }: { row: SessionRow; open: SessionProps["open"]; menu: SessionProps["menu"]; index?: number }) {
  const worktree = placeSub(placeOf(s), s.machine_name);
  return <button data-index={index} className="pv-row" data-key={`${s.machine_id}:${s.id}`} onClick={() => open(s)} onContextMenu={e => menu(e.nativeEvent, s)}><span className="t">{s.title || "Untitled session"}</span><span className="m"><HarnessIcon agent={s.agent} title={agentLabel(s.agent)} /><span data-st={s.status}>{s.status}</span>{(worktree ? [worktree] : [s.machine_name, base(s.cwd)]).filter(Boolean).map((text, i) => <span key={i}>{text}</span>)}</span>{s.last_activity ? <span className="ago">{formatAgo(s.last_activity, "short")}</span> : null}</button>;
}

function SessionList({ rows, all, open, menu }: SessionProps & { all: boolean }) {
  const list = useRef<HTMLDivElement>(null);
  // A short list shows five rows and scrolls the rest.
  const [maxHeight, setMaxHeight] = useState<number | null>(null);
  useLayoutEffect(() => {
    const target = list.current;
    if (!target || all) return;
    if (rows.length <= 5) { setMaxHeight(null); return; }
    const frame = requestAnimationFrame(() => {
      const height = [...target.children].slice(0, 5).reduce((sum, row) => sum + (row instanceof HTMLElement ? row.offsetHeight : 0), 0);
      if (height > 0) setMaxHeight(height);
    });
    return () => cancelAnimationFrame(frame);
  }, [all, rows]);
  const content = rows.map(row => <SessionRowView key={sessionKey(row)} row={row} open={open} menu={menu}/>);
  return all ? <>{content}</> : <div className="pj-scroll" ref={list} style={maxHeight === null ? undefined : { maxHeight }}>{content}</div>;
}

function VirtualSessions({ rows, open, menu }: SessionProps) {
  const list = useRef<HTMLDivElement>(null);
  const focusFrame = useRef<number | null>(null);
  const [parent, setParent] = useState<HTMLElement | null>(null);
  const [offset, setOffset] = useState(0);
  const [pinned, setPinned] = useState<string[]>([]);
  useLayoutEffect(() => () => { if (focusFrame.current !== null) cancelAnimationFrame(focusFrame.current); }, []);
  useLayoutEffect(() => {
    let observer: ResizeObserver | null = null;
    const frame = requestAnimationFrame(() => {
      const target = list.current;
      if (!target) return;
      let scroll: HTMLElement | null = null;
      const measure = () => {
        if (!target.isConnected) return;
        const next = target.closest<HTMLElement>(".pj-det");
        if (next !== scroll) {
          if (scroll) observer?.unobserve(scroll);
          scroll = next; setParent(next);
          if (next) observer?.observe(next);
        }
        if (scroll) setOffset(target.getBoundingClientRect().top - scroll.getBoundingClientRect().top + scroll.scrollTop);
      };
      if (typeof ResizeObserver !== "undefined") {
        observer = new ResizeObserver(measure);
        observer.observe(target);
      }
      measure();
    });
    return () => { cancelAnimationFrame(frame); observer?.disconnect(); };
  }, []);
  useLayoutEffect(() => {
    let focusFrame: number | null = null;
    const update = () => {
      const root = list.current;
      if (!root) return;
      const index = (node: Node | null) => {
        const element = node instanceof Element ? node : node?.parentElement;
        const row = element?.closest<HTMLElement>("[data-index]");
        return row && root.contains(row) ? Number(row.dataset.index) : -1;
      };
      const focus = index(document.activeElement), selection = document.getSelection();
      const start = selection && !selection.isCollapsed ? index(selection.anchorNode) : -1;
      const end = selection && !selection.isCollapsed ? index(selection.focusNode) : -1;
      const keep = new Set<string>();
      if (focus >= 0 && rows[focus]) keep.add(sessionKey(rows[focus]));
      if (start >= 0 && end >= 0) for (let i = Math.min(start, end); i <= Math.max(start, end); i++) if (rows[i]) keep.add(sessionKey(rows[i]));
      const next = [...keep];
      setPinned(previous => previous.length === next.length && previous.every((key, i) => key === next[i]) ? previous : next);
    };
    // Selection is established after focus leaves a row, so let the browser
    // publish the selection before deciding which rows can be evicted.
    const focusOut = () => {
      if (focusFrame !== null) cancelAnimationFrame(focusFrame);
      focusFrame = requestAnimationFrame(() => { focusFrame = null; update(); });
    };
    document.addEventListener("selectionchange", update);
    document.addEventListener("focusin", update);
    document.addEventListener("focusout", focusOut);
    return () => { if (focusFrame !== null) cancelAnimationFrame(focusFrame); document.removeEventListener("selectionchange", update); document.removeEventListener("focusin", update); document.removeEventListener("focusout", focusOut); };
  }, [rows]);
  const indices = useMemo(() => new Map(rows.map((row, index) => [sessionKey(row), index])), [rows]);
  const getItemKey = useCallback((index: number) => sessionKey(rows[index]), [rows]);
  const rangeExtractor = useCallback((range: Range) => {
    const keep = pinned.flatMap(key => { const index = indices.get(key); return index === undefined ? [] : [index]; });
    return [...new Set([...defaultRangeExtractor(range), ...keep])].sort((a, b) => a - b);
  }, [indices, pinned]);
  const virtual = useVirtualizer({ count: rows.length, getScrollElement: () => parent, estimateSize: () => 54,
    getItemKey, overscan: 8, scrollMargin: offset, rangeExtractor, anchorTo: "end", initialRect: { width: 0, height: 600 } });
  return <div ref={list} style={{ height: virtual.getTotalSize(), position: "relative" }} onKeyDown={event => {
    if (event.key !== "Tab" || event.altKey || event.ctrlKey || event.metaKey || !(event.target instanceof HTMLButtonElement)) return;
    if (focusFrame.current !== null) { cancelAnimationFrame(focusFrame.current); focusFrame.current = null; }
    const index = Number(event.target.dataset.index), next = index + (event.shiftKey ? -1 : 1);
    if (!Number.isInteger(index) || next < 0 || next >= rows.length) return;
    event.preventDefault();
    const origin = event.target;
    virtual.scrollToIndex(next, { align: "auto" });
    const focus = (remaining: number) => { focusFrame.current = requestAnimationFrame(() => {
      focusFrame.current = null;
      if (!origin.isConnected || !list.current?.contains(origin) || document.activeElement !== origin) return;
      const target = list.current?.querySelector<HTMLButtonElement>(`button[data-index="${next}"]`);
      if (target?.isConnected) target.focus({ preventScroll: true });
      else if (remaining) focus(remaining - 1);
    }); };
    focus(8);
  }}>{virtual.getVirtualItems().map(item => <div key={item.key} data-index={item.index} ref={virtual.measureElement} style={{ position: "absolute", top: 0, left: 0, width: "100%", transform: `translateY(${item.start - offset}px)`, borderTop: item.index ? "1px solid var(--border)" : undefined, boxSizing: "border-box" }}><SessionRowView row={rows[item.index]} open={open} menu={menu} index={item.index}/></div>)}</div>;
}
