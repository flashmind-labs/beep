// Joining a project's team: the sheet that reads a `beepteam1` code and runs
// its four checks as steps, and the contacts' invites that wait on Home.
import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { flushSync } from "react-dom";
import type { BoardCard } from "../center/lanes-view.tsx";
import { importSheet } from "../center/sidebar.tsx";
import { state } from "../center/state.ts";
import { currentLocation } from "../center/views.tsx";
import { pageShown } from "../ui/active.ts";
import { reason } from "../ui/format.ts";
import { Icon } from "../ui/icons.tsx";
import { Button, SheetHead, Spinner } from "../ui/kit.tsx";
import { openSheet } from "../ui/parts.tsx";
import { isMobile } from "../ui/platform.ts";
import { invoke } from "../ui/tauri.ts";
import { hostLabel, repoPath, type TeamRecord } from "./team-state.ts";
import { readTeam, Who } from "./team-view.tsx";

/// What a code invites to (TicketView).
interface Ticket {
  host: string;
  /// `org/name`.
  repo: string;
  /// The project that has the repository here; null until it is imported.
  project: string | null;
}

/// A contact's invite to their team, waiting for Accept (InviteView).
export interface TeamInvite {
  from: string;
  login: string | null;
  /// `host/org/name`.
  repo: string;
  ticket: string;
}

/// Where the sheet is: waiting for a code, reading it, ready to join, joining, joined, or failed.
type JoinState =
  | { kind: "code" }
  | { kind: "reading"; code: string }
  | { kind: "ready"; code: string; ticket: Ticket }
  | { kind: "joining"; code: string; ticket: Ticket }
  | { kind: "joined"; ticket: Ticket; login: string }
  | { kind: "failed"; code: string; ticket: Ticket | null; message: string };

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
      // An imported repository joins at once: Accept or Join was the go.
      if (ticket.project === null) setJoin({ kind: "ready", code: text, ticket });
      else void run(text, ticket);
    } catch (error) {
      if (live.current) setJoin({ kind: "failed", code: text, ticket: null, message: reason(error) });
    }
  };
  const run = async (text: string, ticket: Ticket) => {
    setJoin({ kind: "joining", code: text, ticket });
    try {
      const record = await invoke<TeamRecord>("team_join", { ticket: text });
      if (invite) await invoke("team_invite_drop", { from: invite.from, repo: invite.repo, ticket: invite.ticket }).catch(error => console.warn("invite:", reason(error)));
      if (ticket.project !== null) void readTeam(ticket.project);
      void readInvites();
      if (live.current) setJoin({ kind: "joined", ticket, login: record.me ?? "" });
    } catch (error) {
      if (live.current) setJoin({ kind: "failed", code: text, ticket, message: reason(error) });
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
      {join.kind === "failed" && <Button look="primary" onClick={() => void (join.ticket ? run(join.code, join.ticket) : read(join.code))}>Try again</Button>}
      <Button look={join.kind === "joined" ? "primary" : "quiet"} onClick={close}>{join.kind === "joined" ? "Done" : "Cancel"}</Button>
    </div>
  </>;
}

function JoinCode({ code, setCode, submit }: { code: string; setCode(code: string): void; submit(): void }) {
  const input = useRef<HTMLInputElement>(null);
  useLayoutEffect(() => { input.current?.focus(); }, []);

  return <>
    <div className="am-os">Paste the code a teammate shared. You need the project imported and push access to it.</div>
    <input ref={input} className="am-code" placeholder="beepteam1…" spellCheck={false} autoComplete="off" aria-label="Team code" value={code}
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

  const machine = ticket.project === null ? null : state.home?.projects?.find(project => project.name === ticket.project)?.machine_name ?? null;
  const later: StepState = join.kind === "joined" ? "done" : join.kind === "joining" ? "running" : "waiting";
  const host = hostLabel(ticket.host);
  const login = join.kind === "joined" && join.login ? ` as ${join.login}` : "";

  return <div className="tm-steps">
    {ticket.project === null
      ? <Step state="failed" text={`Import ${ticket.repo} first`}><Button small onClick={() => { close(); importSheet(`https://${ticket.host}/${ticket.repo}`); }}>Import</Button></Step>
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

/// How often Home reads the contacts' invites while it shows. No event says
/// one arrived, so Home asks, and seldom.
const INVITES_MS = 60_000;
let invites: TeamInvite[] = [];
const listeners = new Set<() => void>();

/// Reads the contacts' invites that wait on this desktop. A phone neither joins nor invites.
export async function readInvites() {
  if (isMobile()) return;

  try {
    invites = await invoke<TeamInvite[]>("team_invites") ?? [];
  } catch (error) {
    console.warn("team invites:", reason(error));
  }

  for (const listener of listeners) listener();
}

/// The invites as last read, read now and every `INVITES_MS` while Home shows.
export function useInvites() {
  const [shown, setShown] = useState(invites);

  useEffect(() => {
    const update = () => setShown(invites);
    listeners.add(update);
    void readInvites();
    const timer = setInterval(() => { if (pageShown() && currentLocation()?.view === "home") void readInvites(); }, INVITES_MS);
    return () => { listeners.delete(update); clearInterval(timer); };
  }, []);

  return shown;
}

/// An invite as a Needs you card: Accept joins with its code, Dismiss drops it.
export function inviteCard(invite: TeamInvite): BoardCard {
  const who = invite.login ?? "A contact", accept = () => openTeamJoin(invite);
  const dismiss = () => {
    invoke("team_invite_drop", { from: invite.from, repo: invite.repo, ticket: invite.ticket }).catch(error => console.warn("invite:", reason(error))).finally(() => void readInvites());
  };

  return { key: `invite:${invite.from}:${invite.repo}`, lane: "needs", project: null, color: null, session: false, lead: null, title: `${who} invites you to ${repoPath(invite.repo)}`,
    sub: invite.repo, foot: null, action: { kind: "several", actions: [{ kind: "run", label: "Accept", primary: true, run: accept }, { kind: "run", label: "Dismiss", primary: false, run: dismiss }] },
    dim: false, theirs: false, hint: null, open: accept, menu: null };
}
