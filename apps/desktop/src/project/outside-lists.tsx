import { useState, type MouseEvent, type ReactNode } from "react";
import { formatAgo, plural } from "../ui/format.ts";
import { Icon } from "../ui/icons.tsx";
import { Button } from "../ui/kit.tsx";
import { itemChips, type GitItemRow } from "../center/openitems-state.ts";
import { agentLabel, foundSessions, type FoundRow } from "./projects-state.ts";

/// The folders the Found list shows before "All".
const FOUND_SHOWN = 5;

/// Open or closed, kept for the window's lifetime. A key not set yet takes its default.
const toggled = new Map<string, boolean>();
function useFold(key: string, initial: boolean) {
  const [, redraw] = useState(0);
  const open = toggled.get(key) ?? initial;
  return [open, () => { toggled.set(key, !open); redraw(n => n + 1); }] as const;
}

/// A click on a button inside a row does its own work and leaves the row alone.
const act = (event: MouseEvent, run: () => void) => { event.stopPropagation(); run(); };

function Fold({ id, title, count, note, children }: { id: string; title: string; count: string; note: string; children: ReactNode }) {
  const [open, toggle] = useFold(id, true);
  return <section className="cc-panel pl"><button type="button" className="pl-head" aria-expanded={open} onClick={toggle}><span className="twist"><Icon name="twist" /></span><span className="lbl">{title}</span><span className="n">{count}</span><span className="note">{note}</span></button>{open && <div className="pl-body">{children}</div>}</section>;
}

export interface ElsewhereRepo { key: string; name: string; failed: string | null; url: string; last: number; importing: boolean; disabled: boolean; primary: boolean; items: GitItemRow[]; import(): void }

function counts(items: GitItemRow[]) {
  const prs = items.filter(item => item.kind === "pull_request").length;
  return [prs && plural(prs, "PR", "PRs"), items.length > prs && plural(items.length - prs, "issue", "issues")].filter(Boolean).join(" · ");
}

function RepoLines({ repo, review, reply, open }: { repo: ElsewhereRepo; open(url: string): void; review(item: GitItemRow): void; reply(item: GitItemRow): void }) {
  const [shown, toggle] = useFold(`repo:${repo.key}`, repo.primary);
  return <>
    <div className="pl-row repo" onClick={toggle}>
      <button type="button" className="t" aria-expanded={shown} title={repo.name}><span className="twist"><Icon name="twist" /></span><b>{repo.name}</b><span className="dim">{repo.failed ?? counts(repo.items)}</span></button>
      <span className="r"><span className="age">{formatAgo(repo.last, "short")}</span><span className="acts"><Button look={repo.primary ? "primary" : "secondary"} small disabled={repo.disabled} onClick={event => act(event, repo.import)}>{repo.importing ? "Importing…" : "Import project"}</Button></span></span>
    </div>
    {shown && repo.items.map(item => <div key={item.url} className="pl-row it" onClick={() => open(item.url)}>
      <span className="ic"><Icon name={item.kind === "pull_request" ? "pr" : "issue"} /></span>
      <button type="button" className="t" title={item.url}><span className="num">#{item.number}</span><b>{item.title}</b>{item.labels?.map(label => <i key={label} className="pj-tag">{label}</i>)}</button>
      <span className="r">{itemChips(item).map((chip, index) => <span className={chip.cls ? `chip ${chip.cls}` : "chip"} key={`${chip.text}:${index}`}>{chip.text}</span>)}<span className="age">{formatAgo(item.updated_at, "short")}</span><span className="acts">{item.kind === "pull_request" && <Button look="quiet" small onClick={event => act(event, () => review(item))}>Review</Button>}<Button look="quiet" small onClick={event => act(event, () => reply(item))}>Reply</Button></span></span>
    </div>)}
  </>;
}

export function ElsewhereSection({ repos, open, review, reply }: { repos: ElsewhereRepo[]; open(url: string): void; review(item: GitItemRow): void; reply(item: GitItemRow): void }) {
  const total = repos.reduce((sum, repo) => sum + repo.items.length, 0);
  return <Fold id="section:elsewhere" title="Your PRs and issues elsewhere" count={`${plural(repos.length, "repo", "repos")} · ${total} open`} note="in repos without a project">
    {repos.map(repo => <RepoLines key={repo.key} repo={repo} open={open} review={review} reply={reply} />)}
  </Fold>;
}

export function FoundSection({ rows, importing, error, importOne, add }: { rows: FoundRow[]; importing: string | null; error: { name: string; message: string } | null; importOne(row: FoundRow): void; add(): void }) {
  return <Fold id="section:found" title="Found on your machines" count={plural(rows.length, "folder", "folders")} note="not projects yet">
    {rows.slice(0, FOUND_SHOWN).map(row => <div key={row.name}>
      <div className="pl-row found">
        <b className="n" title={row.name}>{row.name}</b>
        <span className="where">{row.places.map(place => <span className="chip" key={`${place.machine_name}:${place.folder}`} title={`${place.machine_name}: ${place.folder}`}>{place.machine_name}</span>)}</span>
        <span className="r"><span className="dim">{[...row.agents.map(agent => `${agentLabel(agent.agent)} ${agent.sessions}`), row.last_activity ? formatAgo(row.last_activity, "long") : ""].filter(Boolean).join(" · ")}</span><span className="acts"><Button small disabled={importing !== null} title={`Import ${row.name} with ${foundSessions(row)} sessions`} onClick={() => importOne(row)}>{importing === row.name ? "Importing…" : "Import"}</Button></span></span>
      </div>
      {error?.name === row.name && <div className="pl-bad">{error.message}</div>}
    </div>)}
    {rows.length > FOUND_SHOWN && <Button look="quiet" small onClick={add}>All {rows.length}</Button>}
  </Fold>;
}
