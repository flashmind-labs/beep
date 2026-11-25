// A project's team on its page: the Team page, and teammates' and asks' cards.
import { useEffect, useState } from "react";
import { projectChanged } from "./barstate.ts";
import type { BoardCard, CardAction } from "../center/lanes-view.tsx";
import { formatAgo, plural, reason } from "../ui/format.ts";
import { Icon } from "../ui/icons.tsx";
import { Button, ChipToggle, CopyButton, EmptyPanel, LineRow, NO_AUTO, SectionHead, Switch } from "../ui/kit.tsx";
import { confirmSheet } from "../ui/menu.tsx";
import { invoke, openUrl } from "../ui/tauri.ts";
import { AGENT_ACTIVE } from "../agents/sessionmenu-state.ts";
import { issueSession, projectButtons, startTask } from "./composer.tsx";
import { localProjectId } from "./localid.ts";
import { thr } from "./stores.ts";
import { openTile } from "./tiles.tsx";
import { assign, teamCardsOf, teamOf } from "./team.ts";
import { askWorkPrompt, cardLane, filterCards, needKind, ownedIssues, repoPath, replyPrompt, sameLogin, teammates,
  type BoardFilter, type IssueOwner, type TeamCard, type TeamRecord, type TeamView } from "./team-state.ts";
import type { IssueRow } from "./threads.ts";
import { forgeLinks } from "./projects-state.ts";
import { openTeamJoin, Who, whoColor } from "./teamjoin.tsx";

type Team = Extract<TeamView, { kind: "team" }>;

// ---------- Board ----------

/// The board's filter of each project, by name.
const filters = new Map<string, BoardFilter>();

/// Whose cards project `project`'s board shows.
const boardFilter = (project: string): BoardFilter => filters.get(project) ?? { kind: "everyone" };

/// Whether project `project`'s board shows its filter chips with a teammate
/// or the user picked, so an empty board still offers Everyone.
export function boardFiltered(project: string) {
  const view = teamOf(project);
  return view?.kind === "team" && teammates(view).length > 0 && boardFilter(project).kind !== "everyone";
}

/// A board card with whose it is, for the filter.
export interface OwnedCard { login: string; card: BoardCard }

/// The cards on project `project`'s board: the user's own `mine`, its team's
/// asks and teammates' cards, through the board's filter. `openThread` opens
/// an issue or pull request of the project.
export function teamBoard(project: string, mine: readonly BoardCard[], openThread: (thread: Thread) => void): BoardCard[] {
  const view = teamOf(project);
  if (view?.kind !== "team") return [...mine];

  const team = teamCardsOf(project);
  const owned: OwnedCard[] = [
    ...mine.map(card => ({ login: view.me, card })),
    ...team.needs_you.map(card => ({ login: view.me, card: needCard(project, view, card, openThread) })),
    ...team.own_queued.map(card => ({ login: view.me, card: teamCard(project, card, openThread, false) })),
    ...team.teammates.map(card => ({ login: card.login, card: teamCard(project, card, openThread, true) })),
  ];

  return filterCards(owned, boardFilter(project), view.me).map(owned => owned.card);
}

/// An issue or pull request a team card links to.
export type Thread = { kind: "issue"; number: number } | { kind: "pr"; number: number };

const threadOf = (card: TeamCard): Thread | null => card.pr !== null ? { kind: "pr", number: card.pr } : card.issue !== null ? { kind: "issue", number: card.issue } : null;

const threadLabel = (thread: Thread | null) => thread === null ? null : thread.kind === "pr" ? `PR #${thread.number}` : `#${thread.number}`;

/// A card of the team's board: a teammate's (`theirs`), their login first and
/// a shade dimmer than the user's, or the user's own issue the pickup limit
/// holds back. It opens its issue or pull request, never a session.
function teamCard(project: string, card: TeamCard, openThread: (thread: Thread) => void, theirs: boolean): BoardCard {
  const thread = threadOf(card), lane = cardLane(card.lane);

  return { key: `${theirs ? "mate" : "own"}:${card.login}:${card.id}`, lane, project, color: null, session: false,
    lead: theirs ? <Who login={card.login}/> : <span style={{ display: "contents" }}><Icon name="issue"/></span>, title: card.title,
    sub: [card.machine, threadLabel(thread)].filter(Boolean).join(" · "), foot: card.lane === "queued" ? "queued" : formatAgo(card.updated_at, "since"),
    action: null, dim: false, theirs, hint: null, open: () => { if (thread) openThread(thread); }, menu: null };
}

/// A Needs you card of the team for the user: a teammate's ask, answered by an
/// agent, given to a teammate or dismissed, or an issue whose automatic
/// session ended without a pull request.
function needCard(project: string, view: Team, card: TeamCard, openThread: (thread: Thread) => void): BoardCard {
  const thread = threadOf(card), need = needKind(card);
  const dismiss: CardAction = { kind: "run", label: "Dismiss", primary: false, run: () => void dismissCard(project, card.id) };
  const actions: CardAction[] = need.kind === "ask" ? askActions(project, view, card, need.id) : [];

  return { key: `team:${card.id}`, lane: "needs", project, color: null, session: false, lead: <Who login={card.login}/>, title: card.title,
    sub: need.kind === "ask" ? ["asks you", threadLabel(thread)].filter(Boolean).join(" · ") : [card.machine, threadLabel(thread), "ended without a pull request"].filter(Boolean).join(" · "),
    foot: null, action: { kind: "several", actions: [...actions, dismiss] }, dim: false, theirs: false, hint: card.title,
    open: () => { if (thread) openThread(thread); }, menu: null };
}

/// What answers a teammate's ask: an agent that works on it, one that
/// replies, or giving its issue to a teammate.
function askActions(project: string, view: Team, card: TeamCard, id: string): CardAction[] {
  const agent = (label: string, hint: string, prompt: string): CardAction => ({ kind: "agent", item: projectButtons(`team ${label} ${project}\n${id}`, {
    project, machine: null, choose: true, label, hint, session: null, start: (pick, machine) => startTask(project, { prompt, target: null }, pick, machine),
  }) });
  const actions = [
    agent("Start agent", "Pick a model: an agent works on what they ask", askWorkPrompt(card.login, { id, text: card.title })),
    agent("Reply", "Pick a model: an agent answers them", replyPrompt(card.login, { id, text: card.title })),
  ];
  const issue = card.issue;
  if (issue === null) return actions;

  const logins = view.record.members.map(member => member.login);
  return [...actions, { kind: "menu", label: "Assign to", items: logins.map(login => ({ label: login, on: () => void assign(project, issue, [login]).catch(error => console.warn("assign:", reason(error))) })) }];
}

async function dismissCard(project: string, card: string) {
  try {
    await invoke("team_dismiss", { projectId: localProjectId(project), card });
  } catch (error) {
    console.warn("dismiss:", reason(error));
  }

}

/// The board's filter chips: everyone, the user's, then each teammate's;
/// none while the user is alone on the team.
export function TeamFilter({ project, view }: { project: string; view: Team }) {
  const others = teammates(view);
  if (!others.length) return null;

  const filter = boardFilter(project);
  const pick = (next: BoardFilter) => { filters.set(project, next); projectChanged.notify(); };
  const on = (login: string) => filter.kind === "member" && sameLogin(filter.login, login);

  return <div className="tm-filter">
    <ChipToggle label="Everyone" selected={filter.kind === "everyone"} onToggle={() => pick({ kind: "everyone" })}/>
    <ChipToggle label="Mine" selected={filter.kind === "mine"} onToggle={() => pick({ kind: "mine" })}/>
    {others.map(login => <ChipToggle key={login} label={login} selected={on(login)} onToggle={() => pick({ kind: "member", login })}/>)}
  </div>;
}

// ---------- Issues ----------

/// The Issues list's owner filter of each project, by name.
const owners = new Map<string, IssueOwner>();

/// `issues` through project `project`'s owner filter; all without a team.
export function teamIssues<T extends { assignees: readonly string[] }>(project: string, issues: readonly T[]): T[] {
  const view = teamOf(project);
  return view?.kind === "team" ? ownedIssues(issues, owners.get(project) ?? "all", view.me) : [...issues];
}

/// All, Mine and Unassigned over the Issues list, each with its count; none without a team.
export function IssueOwners({ project, issues }: { project: string; issues: readonly IssueRow[] }) {
  const view = teamOf(project);
  if (view?.kind !== "team") return null;

  const owner = owners.get(project) ?? "all";
  const chip = (each: IssueOwner, label: string) => <ChipToggle label={`${label} ${ownedIssues(issues, each, view.me).length}`} selected={owner === each}
    onToggle={() => { owners.set(project, each); projectChanged.notify(); }}/>;

  return <div className="tm-filter">{chip("all", "All")}{chip("mine", "Mine")}{chip("unassigned", "Unassigned")}</div>;
}

/// Who has an issue and what is going on with it, at the end of its row; nothing without a team.
export function IssueOwnersCell({ project, issue }: { project: string; issue: IssueRow }) {
  const view = teamOf(project);
  if (view?.kind !== "team") return null;

  const team = teamCardsOf(project), own = issueSession(project, issue);
  const card = [...team.own_queued, ...team.teammates].find(each => each.issue === issue.number && each.pr === null);
  const state = own && AGENT_ACTIVE.has(own.status) ? "working" : card?.lane === "queued" ? "queued" : card && cardLane(card.lane) === "working" ? "working" : null;

  return <span className="tm-owners">{issue.assignees.length ? issue.assignees.map(login => <Who key={login} login={login}/>) : <span className="tm-none">unassigned</span>}{state && <span>{state}</span>}</span>;
}

/// How many open issues are the user's, for the issues pill; null without a team.
export function myIssues(project: string, issues: readonly IssueRow[]) {
  const view = teamOf(project);
  return view?.kind === "team" ? ownedIssues(issues, "mine", view.me).length : null;
}

// ---------- Team page ----------

/// The invite code minted last for each project, by name, and when.
const codes = new Map<string, { code: string; at: number }>();

/// How long an invite code lets people join.
const INVITE_MS = 24 * 3_600_000;

/// How much longer a code minted at `at` is valid.
function codeLife(at: number) {
  const now = Date.now(), left = at + INVITE_MS - now;
  return left > 0 ? `valid for ${formatAgo(now - left, "short", now)}` : "expired: make a new one with Invite";
}

/// The Team row's page: members and their machines, the invite code and
/// contacts, the agents' switches and leaving; or starting a team.
export function TeamPage({ project, machines, running }: { project: string; machines: readonly string[]; running: number }) {
  const view = teamOf(project);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const projectId = localProjectId(project);

  const run = (work: () => Promise<unknown>) => {
    setBusy(true);
    setError("");
    work().catch(failure => setError(reason(failure))).finally(() => setBusy(false));
  };
  const invite = () => run(async () => { codes.set(project, { code: await invoke<string>("team_invite", { projectId }), at: Date.now() }); });

  if (!view) return <EmptyPanel spin text="Reading the team…"/>;
  if (view.kind === "none") return <div className="tm-page">
    <EmptyPanel title="No team yet" text={`Invite people who can push to ${project} to share this board and let their agents message yours.`}/>
    {error && <div className="am-msg bad">{error}</div>}
    <div className="am-line"><Button look="primary" disabled={busy} onClick={invite}>Invite</Button><Button look="quiet" onClick={() => openTeamJoin(null)}>Join with a code</Button></div>
  </div>;

  return <div className="tm-page">
    <SectionHead title="Members" count={view.record.members.length}><Button small disabled={busy} onClick={invite}>Invite</Button></SectionHead>
    <Members project={project} view={view} machines={machines} running={running} remove={login => confirmSheet({ title: `Remove ${login}?`, text: `${login} no longer sees this board, and their agents can no longer message yours.`, action: "Remove",
      run: async () => { await invoke("team_remove", { projectId, login }); } })}/>
    {error && <div className="am-msg bad">{error}</div>}
    <Invite project={project} repo={view.record.repo} members={view.record.members.map(member => member.mesh)}/>
    <SectionHead title="Agents"/>
    <Settings project={project} record={view.record} machines={machines}/>
    {!!view.record.removed.length && <><SectionHead title="Removed" count={view.record.removed.length}/>
      <div className="cc-panel">{view.record.removed.map(login => <LineRow key={login} dataKey={`removed:${login}`} lead={null} title={login} sub="removed or left"
        right={<Button small disabled={busy} onClick={event => { event.stopPropagation(); run(() => invoke("team_readmit", { projectId, login })); }}>Allow back</Button>} onOpen={() => {}}/>)}</div></>}
    <div className="am-line"><Button look="quiet" danger onClick={() => confirmSheet({ title: "Leave the team?", text: `Your teammates on ${repoPath(view.record.repo)} stop seeing your cards, and their agents can no longer message yours.`, action: "Leave",
      run: async () => { await invoke("team_leave", { projectId }); } })}>Leave team</Button></div>
  </div>;
}

function Members({ project, view, machines, running, remove }: { project: string; view: Team; machines: readonly string[]; running: number; remove(login: string): void }) {
  const { teammates: theirs, online: followed } = teamCardsOf(project);
  // A member's row shows the board with their cards alone.
  const show = (filter: BoardFilter) => { filters.set(project, filter); openTile("overview"); };

  return <div className="cc-panel tm-members">{view.record.members.map(member => {
    const me = sameLogin(member.login, view.me), mine = theirs.filter(card => sameLogin(card.login, member.login));
    const where = me ? machines : [...new Set(mine.map(card => card.machine))];
    const agents = me ? running : mine.filter(card => cardLane(card.lane) === "working" && card.lane !== "queued").length;
    const online = me || followed.some(login => sameLogin(login, member.login));

    return <LineRow key={member.mesh} dataKey={`member:${member.login}`} lead={<span className="tm-face" style={{ background: whoColor(member.login) }}>{member.login.slice(0, 1).toUpperCase()}</span>}
      title={member.login} sub={[me ? "you" : null, where.join(", "), agents ? plural(agents, "agent", "agents") : null].filter(Boolean).join(" · ")}
      right={<>{!me && <Button small look="quiet" className="tm-remove" onClick={event => { event.stopPropagation(); remove(member.login); }}>Remove</Button>}<span className={online ? "ddot" : "ddot off"}/>{online ? "online" : "offline"}</>}
      onOpen={() => show(me ? { kind: "mine" } : { kind: "member", login: member.login })}/>;
  })}</div>;
}

/// A person of another mesh on the user's teams (ContactView).
interface Contact { mesh: string; logins: { repo: string; login: string }[] }

/// The invite code with what it lets in, and the contacts to invite who are not on the team.
function Invite({ project, repo, members }: { project: string; repo: string; members: readonly string[] }) {
  const [contacts, setContacts] = useState<Contact[]>([]);
  const [sent, setSent] = useState<{ kind: "idle" } | { kind: "sending"; mesh: string } | { kind: "sent"; login: string } | { kind: "failed"; message: string }>({ kind: "idle" });
  const code = codes.get(project);

  useEffect(() => {
    let live = true;
    invoke<Contact[]>("contacts").then(found => { if (live) setContacts(found ?? []); }).catch(error => console.warn("contacts:", reason(error)));
    return () => { live = false; };
  }, [project]);

  const others = contacts.filter(contact => !members.includes(contact.mesh) && contact.logins.length);
  const send = (contact: Contact, login: string) => {
    if (sent.kind === "sending") return;
    setSent({ kind: "sending", mesh: contact.mesh });
    invoke("team_invite_contact", { projectId: localProjectId(project), contact: contact.mesh })
      .then(() => setSent({ kind: "sent", login }), error => setSent({ kind: "failed", message: reason(error) }));
  };
  const remove = (contact: Contact, login: string) => confirmSheet({ title: `Remove ${login}?`, text: `${login} is taken off every team you share, and their agents can no longer message yours.`, action: "Remove",
    run: async () => { await invoke("contact_remove", { contact: contact.mesh }); setContacts(now => now.filter(each => each.mesh !== contact.mesh)); } });

  if (!code && !others.length) return null;

  return <><SectionHead title="Invite"/>
    <div className="tm-invite">
      {code && <><div className="tm-code"><code title={code.code}>{code.code}</code><CopyButton text={code.code} small/></div>
        <div className="tm-note">for anyone with push access to {repoPath(repo)} · {codeLife(code.at)}</div></>}
      {!!others.length && <><div className="tm-note">Or invite a contact:</div>
        <div className="tm-contacts">{others.map(contact => {
          const login = contact.logins[0]?.login ?? contact.mesh;
          return <LineRow key={contact.mesh} dataKey={`contact:${login}`} lead={null} title={login} sub={contact.logins.map(each => repoPath(each.repo)).join(", ")} onOpen={() => send(contact, login)}
            right={<><Button small look="quiet" className="tm-remove" onClick={event => { event.stopPropagation(); remove(contact, login); }}>Remove</Button>
              <Button small disabled={sent.kind === "sending"} onClick={event => { event.stopPropagation(); send(contact, login); }}>Invite</Button></>}/>;
        })}</div>
        {sent.kind === "sent" && <div className="tm-note">Invited {sent.login}.</div>}
        {sent.kind === "failed" && <div className="am-msg bad">{sent.message}</div>}</>}
    </div>
  </>;
}

/// The team's switches: picking up issues assigned to the user, at most so
/// many at once, and teammates' agents messaging the user's.
function Settings({ project, record, machines }: { project: string; record: TeamRecord; machines: readonly string[] }) {
  const [limit, setLimit] = useState(record.pickup_limit === null ? "" : String(record.pickup_limit));
  const [error, setError] = useState("");

  const save = ({ pickup = record.pickup, pickupLimit = record.pickup_limit, asks = record.asks }: { pickup?: boolean; pickupLimit?: number | null; asks?: boolean }) => {
    setError("");
    invoke("team_settings", { projectId: localProjectId(project), pickup, pickupLimit, asks }).catch(failure => setError(reason(failure)));
  };
  const saveLimit = () => {
    const n = Number.parseInt(limit, 10);
    const pickupLimit = Number.isFinite(n) && n > 0 ? n : null;
    if (pickupLimit !== record.pickup_limit) save({ pickupLimit });
  };
  const where = machines.length ? ` on ${machines[0]}` : "";

  return <div className="cc-panel">
    <LineRow lead={null} title="Pick up issues assigned to me" onOpen={() => save({ pickup: !record.pickup })}
      sub={`starts an agent per issue${where}${record.pickup_limit ? `, at most ${record.pickup_limit} at a time` : ""}`}
      right={<><input className="ip-field tm-limit" inputMode="numeric" {...NO_AUTO} placeholder="no limit" aria-label="At most at a time" value={limit} onClick={event => event.stopPropagation()}
        onChange={event => setLimit(event.currentTarget.value)} onBlur={saveLimit} onKeyDown={event => { if (event.key === "Enter") saveLimit(); }}/>
        <Switch on={record.pickup} contained label="Pick up issues assigned to me" flip={() => save({ pickup: !record.pickup })}/></>}/>
    <LineRow lead={null} title="Teammates' agents can message mine" sub="replies come from the agent working on that issue" onOpen={() => save({ asks: !record.asks })}
      right={<Switch on={record.asks} contained label="Teammates' agents can message mine" flip={() => save({ asks: !record.asks })}/>}/>
    {error && <div className="am-msg bad">{error}</div>}
  </div>;
}

/// Opens issue or pull request `thread` of project `project` on its page, or
/// on the project's forge page when it is closed.
export function threadOpener(project: string, url: string | null, openTile: (key: string) => void) {
  return (thread: Thread) => {
    const projectId = localProjectId(project);
    const listed = thread.kind === "pr" ? thr.prs(projectId)?.some(pr => pr.number === thread.number) : thr.issues(projectId)?.some(issue => issue.number === thread.number);
    if (listed) return openTile(`${thread.kind}:${thread.number}`);

    const page = forgeLinks(url, null)[0]?.url;
    // GitHub and Forgejo both answer an issue's number for a pull request too.
    if (page) void openUrl(`${page}/issues/${thread.number}`);
  };
}
