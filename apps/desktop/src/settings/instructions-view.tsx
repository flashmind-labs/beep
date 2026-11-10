import type { ReactNode } from "react";
import type { Ago } from "../ui/format.ts";
import type { InstructionsTab } from "../center/state.ts";
import { MAX_BYTES, bytes, sizeLabel, versionLine, mineLayer, type VersionRow } from "./instructions-state.ts";
import { SectionHead } from "../ui/kit.tsx";

export interface InstructionsState {
  tab: InstructionsTab;
  newest: VersionRow | null | undefined;
  draft: string | null;
  note: string;
  saving: boolean;
  error: string;
  versions: VersionRow[] | null | undefined;
  shown: string | null;
}

interface Props {
  state: InstructionsState;
  ago: Ago;
  setTab: (tab: InstructionsTab) => void;
  setDraft: (text: string) => void;
  save: () => void;
  discard: () => void;
  showVersion: (version: number) => void;
  restoreVersion: (version: VersionRow) => void;
}

export function InstructionsView(p: Props) {
  const st = p.state;
  const text = st.draft ?? st.newest?.text ?? "";
  const dirty = st.draft !== null && st.draft !== (st.newest?.text ?? "");
  return <div className="cc-view in-view">
    <div className="in-tabs">{([["editor", "Editor"], ["history", "History"]] as const).map(([key, label]) => <button type="button" className="dbtn tm-tab" key={key} aria-current={st.tab === key ? "true" : undefined} onClick={() => p.setTab(key)}>{label}</button>)}</div>
    {st.newest === undefined ? <div className="cc-panel cc-empty"><b>Loading your instructions…</b></div> : st.tab === "editor" ? <div className="in-cols">
      <Section title="For all agents" sub={sizeLabel(text)}><div className="cc-panel in-ed">
        <div className="in-ver"><span><b>{versionLine(st.newest && { ...st.newest, note: "" })}</b>{st.newest ? ` · ${p.ago(st.newest.saved_at)}` : ""}{st.note ? ` · ${st.note}` : ""}</span><span className="sp">{st.saving ? "Saving…" : dirty ? "Not saved" : st.newest ? "Saved" : ""}</span></div>
        <textarea spellCheck={false} aria-label="Your instructions" placeholder={"# How I work\n\n- Write plainly.\n- Ask before deploying anything."} value={text} onChange={e => p.setDraft(e.currentTarget.value)} onKeyDown={e => { if (e.metaKey && e.key === "s") { e.preventDefault(); p.save(); } }} />
        <div className="in-acts"><button type="button" className="dbtn pri" disabled={st.saving || !dirty || bytes(text) > MAX_BYTES} onClick={p.save}>Save</button>{dirty && <button type="button" className="dbtn quiet" onClick={p.discard}>Discard</button>}<button type="button" className="dbtn" onClick={() => p.setTab("history")}>History</button></div>
        {st.error && <div className="cc-err">{st.error}</div>}
      </div></Section>
      <div className="in-sidecol"><Section title="How agents see it" sub="instruction sources"><div className="cc-panel"><ol className="in-layers">
        <li><b>The agent's own rules</b><small>Built into Claude Code, Codex, Gemini CLI or pi</small></li>
        <li><b>Your text in its file</b><small>Your local CLAUDE.md or AGENTS.md, kept as is</small></li>
        <li className="me"><b>Your instructions</b><small>{mineLayer(st.newest)}</small></li>
        <li><b>Beep's rules</b><small>Session setup and tool instructions supplied by Beep</small></li>
        <li><b>Project</b><small>The repository's CLAUDE.md or AGENTS.md</small></li>
        <li><b>Task</b><small>The prompt Beep starts the agent with</small></li>
      </ol></div></Section></div>
    </div> : <History {...p} />}
  </div>;
}

function Section({ title, sub, children }: { title: string; sub: string; children: ReactNode }) {
  return <div><SectionHead title={title}>{sub && <small>{sub}</small>}</SectionHead>{children}</div>;
}

function History(p: Props) {
  const { state: st } = p;
  if (st.versions === undefined) return <div className="cc-panel cc-empty"><b>Loading the versions…</b></div>;
  if (st.versions === null) return <div className="cc-err">{st.error || "Could not load the versions."}</div>;
  if (!st.versions.length) return <div className="cc-panel cc-empty"><b>No versions yet</b><span>Each save makes one.</span></div>;
  return <Section title="History" sub="newest first"><div className="cc-panel">{st.versions.map(v => {
    const open = st.shown === `v${v.version}`;
    return <div className="in-hist" key={v.version}><div className="h"><b>{versionLine(v)}</b><small>{p.ago(v.saved_at)}</small><span className="acts"><button type="button" className="dbtn small" onClick={() => p.showVersion(v.version)}>{open ? "Hide" : "Show"}</button>{v.version !== st.newest?.version && <button type="button" className="dbtn small" onClick={() => p.restoreVersion(v)}>Restore</button>}</span></div>{open && <pre className="in-pre">{v.text}</pre>}</div>;
  })}</div></Section>;
}
