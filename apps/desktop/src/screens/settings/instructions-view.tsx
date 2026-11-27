import type { ReactNode } from "react";
import type { Ago } from "../../ui/format.ts";
import type { InstructionsTab } from "../../stores/center/state.ts";
import { MAX_BYTES, SUGGESTIONS, bytes, sizeLabel, versionLine, mineLayer, type Suggestion, type VersionRow } from "../../stores/settings/instructions-state.ts";
import { Button, EmptyPanel, NO_AUTO, SectionHead } from "../../ui/kit.tsx";

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
  addSuggestion: (suggestion: Suggestion) => void;
}

export function InstructionsView(p: Props) {
  const st = p.state;
  const text = st.draft ?? st.newest?.text ?? "";
  const dirty = st.draft !== null && st.draft !== (st.newest?.text ?? "");
  return <div className="cc-view in-view">
    <div className="in-tabs">{([["editor", "Editor"], ["history", "History"]] as const).map(([key, label]) => <Button className="tm-tab" key={key} aria-current={st.tab === key ? "true" : undefined} onClick={() => p.setTab(key)}>{label}</Button>)}</div>
    {st.newest === undefined ? <EmptyPanel title="Loading your instructions…" /> : st.tab === "editor" ? <div className="in-cols">
      <Section title="For all agents" sub={sizeLabel(text)}><div className="cc-panel in-ed">
        <div className="in-ver"><span><b>{versionLine(st.newest && { ...st.newest, note: "" })}</b>{st.newest ? ` · ${p.ago(st.newest.saved_at)}` : ""}{st.note ? ` · ${st.note}` : ""}</span><span className="sp">{st.saving ? "Saving…" : dirty ? "Not saved" : st.newest ? "Saved" : ""}</span></div>
        <textarea {...NO_AUTO} aria-label="Your instructions" placeholder={"# How I work\n\n- Write plainly.\n- Ask before deploying anything."} value={text} onChange={e => p.setDraft(e.currentTarget.value)} onKeyDown={e => { if (e.metaKey && e.key === "s") { e.preventDefault(); p.save(); } }} />
        <div className="in-acts"><Button look="primary" disabled={st.saving || !dirty || bytes(text) > MAX_BYTES} onClick={p.save}>Save</Button>{dirty && <Button look="quiet" onClick={p.discard}>Discard</Button>}<Button onClick={() => p.setTab("history")}>History</Button></div>
        {st.error && <div className="cc-err">{st.error}</div>}
      </div></Section>
      <div className="in-sidecol"><Section title="How agents see it" sub="instruction sources"><div className="cc-panel"><ol className="in-layers">
        <li><b>The agent's own rules</b><small>Built into Claude Code, Codex or pi</small></li>
        <li><b>Your text in its file</b><small>Your local CLAUDE.md or AGENTS.md, kept as is</small></li>
        <li className="me"><b>Your instructions</b><small>{mineLayer(st.newest)}</small></li>
        <li><b>Beep's rules</b><small>Session setup and tool instructions supplied by Beep</small></li>
        <li><b>Project</b><small>The repository's CLAUDE.md or AGENTS.md</small></li>
        <li><b>Task</b><small>The prompt Beep starts the agent with</small></li>
      </ol></div></Section>
      <Section title="Suggestions" sub="add, then save"><div className="cc-panel"><ul className="in-sugs">{SUGGESTIONS.map(s => {
        const added = text.includes(s.text);
        return <li key={s.title}><span><b>{s.title}</b><small>{s.about}</small></span><Button small disabled={added} onClick={() => p.addSuggestion(s)}>{added ? "Added" : "Add"}</Button></li>;
      })}</ul></div></Section></div>
    </div> : <History {...p} />}
  </div>;
}

function Section({ title, sub, children }: { title: string; sub: string; children: ReactNode }) {
  return <div><SectionHead title={title}>{sub && <small>{sub}</small>}</SectionHead>{children}</div>;
}

function History(p: Props) {
  const { state: st } = p;
  if (st.versions === undefined) return <EmptyPanel title="Loading the versions…" />;
  if (st.versions === null) return <div className="cc-err">{st.error || "Could not load the versions."}</div>;
  if (!st.versions.length) return <EmptyPanel title="No versions yet" text="Each save makes one." />;
  return <Section title="History" sub="newest first"><div className="cc-panel">{st.versions.map(v => {
    const open = st.shown === `v${v.version}`;
    return <div className="in-hist" key={v.version}><div className="h"><b>{versionLine(v)}</b><small>{p.ago(v.saved_at)}</small><span className="acts"><Button small onClick={() => p.showVersion(v.version)}>{open ? "Hide" : "Show"}</Button>{v.version !== st.newest?.version && <Button small onClick={() => p.restoreVersion(v)}>Restore</Button>}</span></div>{open && <pre className="in-pre">{v.text}</pre>}</div>;
  })}</div></Section>;
}
