import { useLayoutEffect, useRef } from "react";
import { ago, base } from "../ui/format.ts";
import { harnessInner, harnessMark } from "../ui/harnessicon.ts";
import { placeOf } from "../agents/place.ts";
import { placeSub } from "../agents/place-state.ts";
import { agentLabel, type SessionRow } from "./projects-state.ts";

export function ProjectSessions({ rows, all, open, menu }: { rows: SessionRow[]; all: boolean; open: (row: SessionRow) => void; menu: (event: MouseEvent, row: SessionRow) => void }) {
  const list = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const target = list.current;
    if (!target || all) return;
    if (rows.length <= 5) { target.style.maxHeight = ""; return; }
    const frame = requestAnimationFrame(() => {
      const height = [...target.children].slice(0, 5).reduce((sum, row) => sum + (row instanceof HTMLElement ? row.offsetHeight : 0), 0);
      if (height > 0) target.style.maxHeight = `${height}px`;
    });
    return () => cancelAnimationFrame(frame);
  }, [all, rows]);
  const content = rows.map(s => {
    const worktree = placeSub(placeOf(s), s.machine_name);
    const safe = /^[a-z0-9-]+$/.test(s.agent) ? ` ${s.agent}` : "";
    const mark = harnessMark(s.agent);
    return <button key={`${s.machine_id}:${s.agent}:${s.id}`} className="pv-row" data-key={`${s.machine_id}:${s.id}`} onClick={() => open(s)} onContextMenu={e => menu(e.nativeEvent, s)}><span className="t">{s.title || "Untitled session"}</span><span className="m"><span className={`cc-ag${safe}${mark ? " mark" : ""}`} title={agentLabel(s.agent)} dangerouslySetInnerHTML={{ __html: harnessInner(s.agent) }} />{[s.status, ...(worktree ? [worktree] : [s.machine_name, base(s.cwd)]), s.last_activity ? ago(s.last_activity) : ""].filter(Boolean).map((text, i) => <span key={i}>{text}</span>)}</span></button>;
  });
  return all ? <>{content}</> : <div className="pj-scroll" ref={list}>{content}</div>;
}
