// Joining a project's team: the sheet that reads a `beepteam1` code and runs
// its four checks as steps, and the contacts' invites that wait on Home.
import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { flushSync } from "react-dom";
import type { BoardCard } from "../center/lanes-view.tsx";
import { refresh } from "../center/refresh.ts";
import { importSheet, loadFound, openProjects } from "../center/sidebar.tsx";
import { state } from "../center/state.ts";
import { reason } from "../ui/format.ts";
import { Icon } from "../ui/icons.tsx";
import { Button, NO_AUTO, SheetHead, Spinner } from "../ui/kit.tsx";
import { openSheet } from "../ui/parts.tsx";
import { isMobile } from "../ui/platform.ts";
import { invoke, listen } from "../ui/tauri.ts";
import { hostLabel, repoPath, type TeamRecord } from "./team-state.ts";
import { Who } from "./team-view.tsx";

/// What a code invites to (TicketView).
interface Ticket {
  host: string;
  /// `org/name`.
  repo: string;
  /// The project that has the repository here; null until it is imported.
  project: string | null;
}

/// The project prepared for a team join (PreparedProject).
interface PreparedProject {
  project: string;
  machine_name: string | null;
  folder: string | null;
  added: boolean;
  cloned: boolean;
}

/// A contact's invite to their team, waiting for Accept (InviteView).
export interface TeamInvite {
  from: string;
  login: string | null;
  /// `host/org/name`.
  repo: string;
  ticket: string;
}

/// Where the sheet is: waiting for a code, reading it, adding the project, joining, joined, or failed.
type JoinState =
  | { kind: "code" }
  | { kind: "reading"; code: string }
  | { kind: "importing"; code: string; ticket: Ticket }
  | { kind: "joining"; code: string; ticket: Ticket; machine: string | null }
  | { kind: "joined"; ticket: Ticket; machine: string | null; login: string }
  | { kind: "failed"; code: string; ticket: Ticket | null; machine: string | null; stage: "read" | "import" | "join"; message: string };

const PRIVACY = "Teammates see your cards on this project's board and can message your agents. They never see your sessions, files, terminals or other projects.";

let open: ReturnType<typeof openSheet> | null = null;

/// Opens the Join sheet, with the code of `invite` when one was accepted.
export function openTeamJoin(invite: TeamInvite | null) {
  if (open) return;
  const label = joinTitle(invite, null);
  const sheet = open = openSheet(label, "am-sheet tm-join", { onClose: () => { open = null; } });
  // Named again once the code is read: the same sheet, so it keeps its state.
  const draw = (named: string) => sheet.render(<JoinSheet invite={invite} close={sheet.close} name={draw}/>, named);
  flushSync(() => draw(label));
}

/// The sheet's title: the repository once the code is read, else the invite's.
function joinTitle(invite: TeamInvite | null, ticket: Ticket | null) {
  if (ticket) return `Join ${ticket.repo}`;
  return invite ? `Join ${repoPath(invite.repo)}` : "Join a team";
}

function JoinSheet({ invite, close, name }: { invite: TeamInvite | null; close(): void; name(label: string): void }) {
  const [join, setJoin] = useState<JoinState>(invite ? { kind: "reading", code: invite.ticket } : { kind: "code" });
  const [code, setCode] = useState("");
  const live = useRef(true);
  useEffect(() => { live.current = true; return () => { live.current = false; }; }, []);

  const read = async (text: string) => {
    setJoin({ kind: "reading", code: text });
    try {
      const ticket = await invoke<Ticket>("team_ticket", { ticket: text });
      if (!live.current) return;
      name(joinTitle(invite, ticket));
      if (ticket.project === null) void prepareAndJoin(text, ticket);
      else void run(text, ticket, null);
    } catch (error) {
      if (live.current) setJoin({ kind: "failed", code: text, ticket: null, machine: null, stage: "read", message: reason(error) });
    }
  };
  const prepareAndJoin = async (text: string, ticket: Ticket) => {
    setJoin({ kind: "importing", code: text, ticket });
    try {
      const prepared = await invoke<PreparedProject>("team_prepare", { ticket: text });
      if (!live.current) return;
      if (prepared.added) {
        void loadFound();
        void refresh();
      }
      void run(text, { ...ticket, project: prepared.project }, prepared.machine_name);
    } catch (error) {
      if (live.current) setJoin({ kind: "failed", code: text, ticket, machine: null, stage: "import", message: reason(error) });
    }
  };
  const run = async (text: string, ticket: Ticket, machine: string | null) => {
    setJoin({ kind: "joining", code: text, ticket, machine });
    try {
      const record = await invoke<TeamRecord>("team_join", { ticket: text });
      if (invite) await invoke("team_invite_drop", { from: invite.from, repo: invite.repo, ticket: invite.ticket }).catch(error => console.warn("invite:", reason(error)));
      if (live.current) setJoin({ kind: "joined", ticket, machine, login: record.me ?? "" });
    } catch (error) {
      if (live.current) setJoin({ kind: "failed", code: text, ticket, machine, stage: "join", message: reason(error) });
    }
  };

  useLayoutEffect(() => { if (invite) void read(invite.ticket); }, []);

  return <>
    <SheetHead title={joinTitle(invite, "ticket" in join ? join.ticket : null)}/>
    {invite?.login && <div className="am-os">Invited by <Who login={invite.login}/></div>}
    {join.kind === "code"
      ? <JoinCode code={code} setCode={setCode} submit={() => { if (code.trim()) void read(code.trim()); }}/>
      : <Steps join={join} close={close}/>}
    <div className="tm-note">{PRIVACY}</div>
    <div className="am-line tm-end">
      {join.kind === "code" && <Button look="primary" disabled={!code.trim()} onClick={() => void read(code.trim())}>Join</Button>}
      {join.kind === "failed" && <Button look="primary" onClick={() => {
        if (join.stage === "import" && join.ticket) void prepareAndJoin(join.code, join.ticket);
        else if (join.ticket) void run(join.code, join.ticket, join.machine);
        else void read(join.code);
      }}>Try again</Button>}
      {join.kind === "joined" && join.ticket.project && <Button look="quiet" onClick={() => { close(); openProjects(join.ticket.project); }}>Open project</Button>}
      <Button look={join.kind === "joined" ? "primary" : "quiet"} onClick={close}>{join.kind === "joined" ? "Done" : "Cancel"}</Button>
    </div>
  </>;
}

function JoinCode({ code, setCode, submit }: { code: string; setCode(code: string): void; submit(): void }) {
  const input = useRef<HTMLInputElement>(null);
  useLayoutEffect(() => { input.current?.focus(); }, []);

  return <>
    <div className="am-os">Paste the code a teammate shared. Beep adds the project automatically if you do not have it yet. You need push access to it.</div>
    <input ref={input} className="am-code" placeholder="beepteam1…" {...NO_AUTO} aria-label="Team code" value={code}
      onChange={event => setCode(event.currentTarget.value)} onKeyDown={event => { if (event.key === "Enter") submit(); }}/>
  </>;
}

/// One check of the join: done, running, waiting, or what to do first.
type StepState = "done" | "running" | "waiting" | "failed";

/// The four checks: the repository is here, the forge sign-in, the push proof, and saving.
function Steps({ join, close }: { join: Exclude<JoinState, { kind: "code" }>; close(): void }) {
  if (join.kind === "reading") return <div className="am-wait"><Spinner/>Reading the code</div>;

  const ticket = join.ticket;
  if (!ticket) return <div className="am-msg bad">{join.kind === "failed" ? join.message : ""}</div>;

  const machine = ("machine" in join && join.machine)
    || (ticket.project === null ? null : state.home?.projects?.find(project => project.name === ticket.project)?.machine_name ?? null);
  const later: StepState = join.kind === "joined" ? "done" : join.kind === "joining" ? "running" : "waiting";
  const host = hostLabel(ticket.host);
  const login = join.kind === "joined" && join.login ? ` as ${join.login}` : "";

  return <div className="tm-steps">
    {join.kind === "importing"
      ? <Step state="running" text={`Adding ${ticket.repo}`}/>
      : join.kind === "failed" && join.stage === "import"
        ? <Step state="failed" text={`Could not add ${ticket.repo}`}><Button small onClick={() => { close(); importSheet(`https://${ticket.host}/${ticket.repo}`); }}>Import manually</Button></Step>
        : <Step state="done" text={machine ? `${ticket.repo} is on ${machine}` : `${ticket.repo} is imported`}/>}
    <Step state={later} text={`Signed in to ${host}${login}`}/>
    <Step state={later === "running" ? "waiting" : later} text={`Proving push access to ${ticket.repo}`}/>
    <Step state={later === "running" ? "waiting" : later} text="Saving the team and contacts"/>
    {join.kind === "failed" && <div className="am-msg bad">{join.message}</div>}
  </div>;
}

function Step({ state, text, children = null }: { state: StepState; text: string; children?: ReactNode }) {
  const mark = state === "running" ? <Spinner/> : <span className={`tm-mark ${state}`}>{state === "done" ? <Icon name="check"/> : state === "failed" ? "!" : "·"}</span>;
  return <div className={`tm-step ${state}`}>{mark}<span>{text}</span>{children}</div>;
}

// ---------- Invites ----------

let invites: TeamInvite[] = [];
/// Whether the invites were read or pushed once.
let known = false;
const listeners = new Set<() => void>();

function takeInvites(list: TeamInvite[]) {
  invites = list;
  known = true;
  for (const listener of listeners) listener();
}

/// Reads the contacts' invites that wait on this desktop, once; the pushes follow. A phone neither joins nor invites.
async function readInvites() {
  if (isMobile()) return;

  try {
    const list = await invoke<TeamInvite[]>("team_invites") ?? [];
    if (!known) takeInvites(list);
  } catch (error) {
    console.warn("team invites:", reason(error));
  }
}

listen<TeamInvite[]>("team-invites", ({ payload }) => takeInvites(payload));

/// The invites as last read or pushed, read when Home first needs them.
export function useInvites() {
  const [shown, setShown] = useState(invites);

  useEffect(() => {
    const update = () => setShown(invites);
    listeners.add(update);
    if (known) update(); else void readInvites();
    return () => { listeners.delete(update); };
  }, []);

  return shown;
}

/// An invite as a Needs you card: Accept joins with its code, Dismiss drops it.
export function inviteCard(invite: TeamInvite): BoardCard {
  const who = invite.login ?? "A contact", accept = () => openTeamJoin(invite);
  const dismiss = () => {
    invoke("team_invite_drop", { from: invite.from, repo: invite.repo, ticket: invite.ticket }).catch(error => console.warn("invite:", reason(error)));
  };

  return { key: `invite:${invite.from}:${invite.repo}`, lane: "needs", project: null, color: null, session: false, lead: null, title: `${who} invites you to ${repoPath(invite.repo)}`,
    sub: invite.repo, foot: null, action: { kind: "several", actions: [{ kind: "run", label: "Accept", primary: true, run: accept }, { kind: "run", label: "Dismiss", primary: false, run: dismiss }] },
    dim: false, theirs: false, hint: null, open: accept, menu: null };
}
