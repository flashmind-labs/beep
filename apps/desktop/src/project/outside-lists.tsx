import type { ProjectWindowProps } from "./projectwindow.ts";
import { useState, type MouseEvent, type ReactNode } from "react";
import { formatAgo, plural } from "../ui/format.ts";
import { Icon } from "../ui/icons.tsx";
import { AgentButton, type AgentButtonItem } from "../review/feedback-view.tsx";
import { Button } from "../ui/kit.tsx";
import { openMenu, type MenuItem } from "../ui/menu.tsx";
import { isMobile } from "../ui/platform.ts";
import { itemChips, itemTime, type GitItemRow, type GitSort } from "../center/openitems-state.ts";
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
  return <section className="cc-panel pl"><button type="button" className="pl-head" aria-expanded={open} onClick={toggle}><span className="twist"><Icon name="twist" /></span><span className="lbl">{title}</span><span className="n">{count}</span>{note && <span className="note">{note}</span>}</button>{open && <div className="pl-body">{children}</div>}</section>;
}

/// A repository's line: `open` unfolds its items at first, `action` is its one button.
/// `start` starts an agent on an item; only a project's items have it.
export interface RepoLine { key: string; name: string; failed: string | null; last: number; open: boolean; items: GitItemRow[]; action: { windowProps?: ProjectWindowProps; label: string; primary?: boolean; disabled?: boolean; run(): void }; start: ItemStart | null }
/// Continue and Review, agent buttons that start at once, and the item's menu.
interface ItemStart { continue(item: GitItemRow): AgentButtonItem; review(item: GitItemRow): AgentButtonItem; menu(item: GitItemRow): MenuItem[] }

function counts(items: GitItemRow[]) {
  const prs = items.filter(item => item.kind === "pull_request").length;
  return [prs && plural(prs, "PR", "PRs"), items.length > prs && plural(items.length - prs, "issue", "issues")].filter(Boolean).join(" · ");
}

function RepoLines({ repo, open }: { repo: RepoLine; open(url: string): void }) {
  const [shown, toggle] = useFold(`repo:${repo.key}`, repo.open);
  return <>
    <div className="pl-row repo" onClick={toggle}>
      <button type="button" className="t" aria-expanded={shown} title={repo.name}><span className="twist"><Icon name="twist" /></span><b>{repo.name}</b><span className="dim">{repo.failed ?? counts(repo.items)}</span></button>
      <span className="r"><span className="age">{formatAgo(repo.last, "short")}</span><span className="acts"><Button {...repo.action.windowProps?.clicks} {...repo.action.windowProps?.menu} look={repo.action.primary ? "primary" : "secondary"} small disabled={repo.action.disabled} onClick={event => act(event, repo.action.run)}>{repo.action.label}</Button></span></span>
    </div>
    {shown && repo.items.map(item => <ItemLine key={item.url} item={item} repo={null} at={item.updated_at} start={repo.start} open={open} />)}
  </>;
}

/// An open PR or issue, with its age from `at`. `repo` names its repository when the list mixes them.
function ItemLine({ item, repo, at, start, open }: { item: GitItemRow; repo: string | null; at: number; start: ItemStart | null; open(url: string): void }) {
  // Desktop hides a row's actions until hover or focus; mounting them on first interaction
  // avoids launching PR session lookups and review scans for hundreds of untouched rows.
  const [activated, setActivated] = useState(false);
  const showActs = start && !activated ? () => setActivated(true) : undefined;
  const renderActs = !!start && (activated || isMobile());

  return <div className={repo ? "pl-row it flat" : "pl-row it"} onClick={() => open(item.url)} onPointerEnter={showActs} onMouseEnter={showActs} onFocus={showActs}>
    <span className="ic"><Icon name={item.kind === "pull_request" ? "pr" : "issue"} /></span>
    <button type="button" className="t" title={item.url}>{repo && <span className="dim">{repo}</span>}<span className="num">#{item.number}</span><b>{item.title}</b>{item.labels?.map(label => <i key={label} className="pj-tag">{label}</i>)}</button>
    <span className="r">{itemChips(item).map((chip, index) => <span className={chip.cls ? `chip ${chip.cls}` : "chip"} key={`${chip.text}:${index}`}>{chip.text}</span>)}<span className="age">{at ? formatAgo(at, "short") : ""}</span>{renderActs && <span className="acts">{item.kind === "pull_request" && <span style={{ display: "contents" }} onClick={event => event.stopPropagation()}>{item.authored && <AgentButton item={start.continue(item)} />}<AgentButton item={start.review(item)} /></span>}<Button look="quiet" small aria-label={`More for #${item.number}`} aria-haspopup="menu" onClick={event => act(event, () => openMenu(event.nativeEvent, `#${item.number}`, start.menu(item)))}>&#8943;</Button></span>}</span>
  </div>;
}

/// Open PRs and issues by repository: a line per repository, which unfolds to its items.
/// A `sort` other than `default` lists every item in one run, newest first.
export function RepoSection({ id, title, note, repos, sort, open }: { id: string; title: string; note: string; repos: RepoLine[]; sort: GitSort; open(url: string): void }) {
  const total = repos.reduce((sum, repo) => sum + repo.items.length, 0);
  const items = sort === "default" ? [] : repos.flatMap(repo => repo.items.map(item => ({ item, repo, at: itemTime(sort, item) }))).sort((a, b) => b.at - a.at);
  return <Fold id={id} title={title} count={`${plural(repos.length, "repo", "repos")} · ${total} open`} note={note}>
    {sort === "default"
      ? repos.map(repo => <RepoLines key={repo.key} repo={repo} open={open} />)
      : items.map(({ item, repo, at }) => <ItemLine key={item.url} item={item} repo={repo.name} at={at} start={repo.start} open={open} />)}
  </Fold>;
}

/// Open PRs and issues as a card per repository, packed into two columns: its name, counts
/// and button, then its items, newest first by `sort`. A card with many scrolls them.
export function RepoCards({ repos, sort, open }: { repos: RepoLine[]; sort: GitSort; open(url: string): void }) {
  return <div className="bd-projects">{repos.map(repo => {
    const items = sort === "default" ? repo.items : [...repo.items].sort((a, b) => itemTime(sort, b) - itemTime(sort, a));

    return <section className="cc-panel pl" key={repo.key}>
      <div className="pl-row repo">
        <span className="t"><b title={repo.name}>{repo.name}</b><span className="dim">{repo.failed ?? counts(repo.items)}</span></span>
        <span className="r"><span className="age">{formatAgo(repo.last, "short")}</span><span className="acts"><Button {...repo.action.windowProps?.clicks} {...repo.action.windowProps?.menu} look={repo.action.primary ? "primary" : "secondary"} small disabled={repo.action.disabled} onClick={repo.action.run}>{repo.action.label}</Button></span></span>
      </div>
      <div className="pl-body">{items.map(item => <ItemLine key={item.url} item={item} repo={null} at={itemTime(sort, item)} start={repo.start} open={open} />)}</div>
    </section>;
  })}</div>;
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
