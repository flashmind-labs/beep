import { HarnessIcon, Icon } from "../ui/icons.tsx";
import { taskAction, taskMenu, taskMeta, taskSummary, workerFolder, workerLine, workerMenu, type ProjectView, type RowActs } from "./agent.ts";

export function ProjectTasks({ view, act }: { view: ProjectView; act: RowActs }) {
  const titles = new Map(view.tasks.map(t => [t.id, t.title]));
  const live = view.workers.filter(w => w.live).length;
  const summary = taskSummary(view.tasks);
  return <>
    <div id="sec-tasks"><div className="cc-sec"><h4>Tasks</h4>{!!summary && <small>{summary}</small>}</div>
      {view.tasks.length ? <div className="cc-panel">{view.tasks.map(t => {
        const action = taskAction(t.state), busy = act.busy(t.id), error = act.error(t.id);
        const diff = t.branch && t.state !== "dropped" ? act.diff?.(t.branch) : null;
        return <div key={t.id} className="pj-task" data-task={t.id} onContextMenu={e => taskMenu(e.nativeEvent, t, act, diff)}><span className="t" title={t.title}>{t.title}</span><span className={`chip ${t.state}`}>{t.state === "running" && <span className="drun" />}{t.state}</span><span className="m">{taskMeta(t, view.tasks, view.workers).map((m, i) => m.url ? <a key={i} href={m.url} onClick={e => { e.preventDefault(); if (m.url) act.openUrl(m.url); }}>{m.text}</a> : m.code ? <code key={i}>{m.text}</code> : <span key={i}>{m.text}</span>)}</span><span className="acts">
          {t.state === "review" && t.branch && act.review && <button type="button" className="dbtn pri small" onClick={() => { if (t.branch) act.review?.(t.branch); }}>Review</button>}
          {t.state === "review" && t.branch && t.pr_url && act.merge && <button type="button" className="dbtn small" title="Merge always asks first" disabled={busy} onClick={() => act.merge?.(t)}>{busy ? "Asking…" : "Merge…"}</button>}
          {t.state === "review" && t.pr_url && <button type="button" className="dbtn small" onClick={() => { if (t.pr_url) act.openUrl(t.pr_url); }}>Open PR</button>}
          {diff && <button type="button" className="dbtn small" onClick={diff}>View diff</button>}
          <button type="button" className="dbtn quiet small" disabled={busy} onClick={() => act.change(t, action.change)}>{busy ? "…" : action.label}</button>{error && <span className="cc-err">{error}</span>}</span></div>;
      })}</div> : <div className="cc-panel cc-empty"><span>No tasks yet. Ask the project agent below and it plans them.</span></div>}
    </div>
    <div id="sec-workers"><div className="cc-sec"><h4>Workers</h4><small>{live ? `${live} running` : "none running"}</small></div><div className="cc-panel pj-tree"><div className="pj-node"><span className="cc-ag beep"><Icon name="beep" /></span><span className="t"><b>Project agent</b><span>Plans tasks, files issues, places workers</span></span></div>{view.workers.map(w => {
      const title = titles.get(w.task_id) ?? null, name = title || workerFolder(w.folder), canOpen = !!w.session && !!act.openWorker, busy = act.busy(w.id), error = act.error(w.id);
      return <div key={w.id} className={`pj-node child${canOpen ? " link" : ""}${w.live ? "" : " over"}`} title={canOpen ? "Open its session" : undefined} onClick={canOpen ? () => act.openWorker?.(w, title) : undefined} onContextMenu={e => workerMenu(e.nativeEvent, w, name, title, act)}><HarnessIcon agent={w.agent} /><span className="t"><b>{name}</b><span title={w.folder}>{workerLine(w, Date.now())}</span></span>{w.live && <span className="acts"><button type="button" className="dbtn quiet small" disabled={busy} onClick={e => { e.stopPropagation(); act.stop(w); }}>{busy ? "Stopping…" : "Stop"}</button>{error && <span className="cc-err">{error}</span>}</span>}{w.last_output && <span className="out" title={w.last_output}>{w.last_output}</span>}</div>;
    })}</div></div>
  </>;
}
