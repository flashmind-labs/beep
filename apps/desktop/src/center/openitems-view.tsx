import { formatAgo } from "../ui/format.ts";
import { Icon } from "../ui/icons.tsx";
import { LabelGroup } from "../project/split-view.tsx";
import { arranged, issueSelection, setIssueSelection } from "../project/openlist.ts";
import { OpenListControls, OpenListStart } from "../project/openlist-view.tsx";
import { labelFilter, type LabelFilter } from "../project/split.ts";
import { reviewStart, itemChips, newestFirst, projectsWithOpen, type GitItemRow, type GitItemsView } from "./openitems-state.ts";
import { draw } from "./home.ts";
import { importTitle, openUrl, quickImport, replyItem, rowImports, type OpenRepo } from "./openitems.ts";
import { projectRows } from "./sidebar.ts";
import { state } from "./state.ts";
import { go } from "./views.ts";
import { Button, Checkbox, Chip, Spinner, SectionHead, useLazyCount } from "../ui/kit.tsx";

function Import({ repo }: { repo: OpenRepo }) {
  const current = rowImports.get(repo.key);
  if (current === "busy") return <span className="age"><Spinner /> Importing…</span>;
  return <>{current && <><span className="chip bad" title={current.error}>Import failed</span>{" "}</>}<Button small title={importTitle()} onClick={event => { event.stopPropagation(); void quickImport(repo); }}>Import</Button></>;
}
function Item({ item, repo, filter, list }: { item: GitItemRow; repo: OpenRepo | null; filter: LabelFilter; list: string }) {
  const selected = item.kind === "issue" ? issueSelection(list, item.url) : null;
  const project = repo?.project ?? null;
  const act = (event: React.MouseEvent, action: () => void) => { event.stopPropagation(); action(); };
  return <div className="cc-row cc-line cc-gr it" onClick={() => openUrl(item.url)}>{selected !== null && <label className="pick"><Checkbox label="Pick this issue" checked={selected} contained change={value => setIssueSelection(list, item.url, value, draw)} /></label>}<span className="ic"><Icon name={item.kind === "pull_request" ? "pr" : "issue"} /></span><button type="button" className="t" title={item.url}><span className="num">#{item.number}</span><b>{item.title}</b><span className="pj-proj">{item.repo}</span>{item.labels?.map(label => <i key={label} className={`pj-tag lab${label === filter.on ? " on" : ""}`} onClick={event => act(event, () => filter.pick(label))}>{label}</i>)}</button><span className="r">{itemChips(item).map((chip, index) => <Chip kind={chip.cls} text={chip.text} key={`${chip.text}:${index}`} />)}<span className="age">{formatAgo(item.updated_at, "short")}</span>{item.kind === "pull_request" && <Button small onClick={event => act(event, () => { go("pane", { agentnew: reviewStart(item, project, state.home?.machines?.filter(m => m.can_host !== false)) }); })}>Review</Button>}<Button small onClick={event => act(event, () => replyItem(item))}>Reply</Button>{repo && !repo.project && <Import repo={repo} />}</span></div>;
}
function LazyItems({ items, repos, filter, list }: { items: GitItemRow[]; repos: Map<string, OpenRepo>; filter: LabelFilter; list: string }) {
  const { count, height, end, first } = useLazyCount<HTMLDivElement>(items.length, 12, 12, row => row.firstElementChild ?? row);
  return <><div ref={first} style={{ display: "contents" }}>{items.slice(0, count).map(item => <Item key={item.url} item={item} repo={repos.get(`${item.host} ${item.repo}`) ?? null} filter={filter} list={list} />)}</div>{count < items.length && <div ref={end} style={{ gridColumn: "1 / -1", height: height ? `${(items.length - count) * height}px` : undefined }} />}</>;
}
function Column({ kind, title, none, view }: { kind: GitItemRow["kind"]; title: string; none: string; view: GitItemsView }) {
  const key = `home:${kind}`, issues = kind === "issue";
  const { items, labels } = labelFilter(key, view.items.filter(item => item.kind === kind).sort(newestFirst), item => item.labels, draw);
  const shown = arranged(key, items, item => item.opened_at);
  const repos = new Map(projectsWithOpen({ items }, projectRows() || []).map(repo => [repo.key, repo]));
  const entries = shown.map(item => ({ key: item.url, project: repos.get(`${item.host} ${item.repo}`)?.project?.name ?? null, number: item.number, title: item.title, url: item.url }));
  return <div><SectionHead title={title} count={shown.length} /><div className="cc-panel cc-open"><OpenListControls list={key} redraw={draw} issues={issues} />{!!labels.all.length && <LabelGroup filter={labels} />}{shown.length ? <LazyItems items={shown} repos={repos} filter={labels} list={key} /> : <div className="cc-empty"><span>{none}</span></div>}{issues && <OpenListStart list={key} shown={entries} redraw={draw} />}</div></div>;
}
export function HomeGit({ view }: { view: GitItemsView }) {
  return <div className="cc-git" id="sec-git">{[state.gitError, ...view.hosts.map(host => host.error)].filter(Boolean).map((error, index) => <div className="cc-note" key={index}><span className="ddot bad" /><span>{error}</span></div>)}<div className="cc-cols"><Column kind="pull_request" title="Pull requests" none="No open pull requests." view={view} /><Column kind="issue" title="Issues" none="No open issues." view={view} /></div></div>;
}
