import { formatAgo } from "../../ui/format.ts";
import { agentLabel, type SessionRow } from "../../stores/project/projects-state.ts";

export function SessionLink({ session, open }: { session: SessionRow; open: (session: SessionRow) => void }) {
  return <button type="button" className="gh-session-link" onClick={() => open(session)}><b>{session.title || "Untitled session"}</b><span>{agentLabel(session.agent)} · {session.status} · {formatAgo(session.last_activity, "short")}</span></button>;
}

export function HistorySessions({ linked, loading, sessions, open }: { linked: boolean; loading: boolean; sessions: readonly SessionRow[]; open: (session: SessionRow) => void }) {
  return <div className="gh-session-view">{linked ? <><p className="gh-empty">Sessions in this worktree</p>{sessions.length ? sessions.map(session =>
    <SessionLink key={`${session.machine_id}:${session.agent}:${session.id}`} session={session} open={open} />)
    : <p className="gh-empty">{loading ? "Reading sessions…" : "No sessions in this worktree."}</p>}</> : <p className="gh-empty">No recorded session link for this commit. Worktree sessions are listed under Worktrees.</p>}</div>;
}

export function HistoryPath({ path, showAll }: { path: string; showAll: () => void }) {
  return <div className="gh-diff-toolbar"><code>{path}</code><button type="button" onClick={showAll}>Show all files</button></div>;
}
