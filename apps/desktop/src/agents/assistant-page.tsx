// The Assistant page: the assistant's chats, each with its threads under it,
// beside the pane, which shows the open chat or thread or the assistant's
// start composer.
import { go } from "../center/nav.ts";
import { Fragment, type ReactNode, useEffect, useState, useSyncExternalStore } from "react";
import { createStore, useStore } from "zustand";
import { AG, type AgentRow, type AgentSession } from "./agents.ts";
import { assistantRows } from "./assistant-state.ts";
import { chatActivity, chatOf, type chatRail, type Thread, type ThreadDetail, threadsByChat } from "./threads-state.ts";
import { sessionMenu } from "./sessionmenu.ts";
import { agentName, runningKey } from "./running-state.ts";
import { ensureSessions, sessions, sessionsRead } from "./sessions.ts";
import { sessionVisible } from "./closing.ts";
import { S } from "../bar/core.ts";
import type { Location } from "../center/state.ts";
import { shell } from "../center/window-shell.tsx";
import { paneModel } from "../agentpane/media-layout.ts";
import { RailList, Row, RunningStatus } from "../project/split-view.tsx";
import { SideToggle } from "../center/sidebar-shell.tsx";
import { Button, EndedFold } from "../ui/kit.tsx";
import { HarnessIcon } from "../ui/icons.tsx";
import { PersonaTag } from "../project/customagents-view.tsx";
import { formatAgo } from "../ui/format.ts";
import { isMobile } from "../ui/platform.ts";
import { CHATS, WidthGrip } from "../ui/width-grip.tsx";

/// What the Assistant page opens on: its chats (the phone shows only the list), or a new chat.
export type AssistantOpen = "chats" | "new";

/// Where a chat's Back goes, and the page's own on the phone.
export const ASSISTANT_CHATS: Location = { view: "pane", arg: { assistant: "chats" } };

/// The open chat's key, which its row shows as the current item.
const openChat = createStore<{ key: string | null }>(() => ({ key: null }));

/// The page opened on its list, until the pane shows something else.
let listing = false;
/// The row the pane last opened a chat with, which may not be listed yet.
let lastRow: AgentRow | null = null;

/// Shows the Assistant page with the assistant's start composer. Its Back goes to the list
/// on the phone, where the composer covers it, and Home on the desktop, which shows both.
export function openAssistantPage(open: AssistantOpen) {
  listing = open === "chats";
  return import("./agentnew.tsx").then(({ renderAgentNew }) => {
    renderAgentNew("", isMobile() && !listing ? ASSISTANT_CHATS : { view: "home", arg: null }, null, true);
    syncAssistantPage();
  });
}

/// The pane opened `row`'s chat, from the page or anywhere else.
export function openedChat(row: AgentRow) {
  lastRow = row;
  listing = false;
}

/// Works out the page from what the pane shows. A chat started from the assistant's composer is
/// on the page before the session list has it, and the list may hold it a second time as found
/// outside Beep, which does not say it is the assistant: any row that says so wins, the one the
/// chat opened with too. A chat's thread is on the page as well.
export function syncAssistantPage() {
  const open = S.view === "agent" ? AG.open : null;

  if (S.view === "agentnew") return show(AG.form.assistant, !listing, null);
  if (!open) return show(false, false, null);

  const id = { machine_id: open.machine_id, id: open.session };
  const same = (row: AgentRow) => row.machine_id === id.machine_id && row.id === id.id;
  const rows = [...sessions, lastRow, AG.page?.session ?? null].filter((row): row is AgentRow => !!row && same(row));
  const chat = rows.map(row => chatOf(sessions, row)).find(found => !!found);
  const on = rows.length ? !!chat : !!shell.getState().assistant;
  show(on, true, runningKey(id));
}

/// The pane went off screen.
export function leaveAssistantPage() {
  show(false, false, null);
}

function show(on: boolean, picked: boolean, chat: string | null) {
  const now = shell.getState().assistant;
  if (on !== !!now || (on && now?.picked !== picked)) shell.setState({ assistant: on ? { picked } : null });
  openChat.setState({ key: on ? chat : null });
}

/// The session list as a store React reads.
function subscribeSessions(change: () => void) {
  sessionsRead.add(change);
  return () => { sessionsRead.delete(change); };
}

let read = 0;
sessionsRead.add(() => { read += 1; });

/// The assistant's chats: New chat, then the running ones, then the ended ones under their fold,
/// with a grip on their right edge. A chat's threads are its subitems, oldest first, each with
/// what it asks or last said, those that ended under a fold of the chat's own; the chat in hand
/// offers a new one.
export function AssistantList() {
  // A chat with threads lists them in place of the sidebar instead.
  const crowded = useStore(paneModel, s => s.crowded);
  const on = useStore(shell, s => !!s.assistant && !(s.side.rail?.kind === "chat" && !s.side.railHidden)) && !crowded, chat = useStore(openChat, s => s.key);
  useSyncExternalStore(subscribeSessions, () => read);
  const [endedOpen, setEndedOpen] = useState(false);
  /// The chats whose ended threads show.
  const [unfolded, setUnfolded] = useState<ReadonlySet<string>>(new Set());

  useEffect(() => {
    if (on) void ensureSessions().catch(() => {});
  }, [on]);

  if (!on) return <div className="pj-list asst-list" hidden />;
  const listed = sessions.filter(sessionVisible);
  const threadsOf = threadsByChat(listed);
  const { running, ended } = assistantRows(listed);
  const item = (session: AgentSession, title: string, lead: ReactNode, sub: ReactNode, right: ReactNode = null) => <Row key={runningKey(session)} picked={chat} nav={false} openTile={() => {}} item={{
    key: runningKey(session), lead, title, sub,
    right: right ?? (session.status === "ended" ? formatAgo(session.last_activity, "since") : <RunningStatus row={{ kind: "agent", ...session }} />),
    open: () => goChat(session),
    // Also how a phone, whose list hides while a chat shows, reaches New thread.
    menu: event => void sessionMenu(event, session, goChat),
  }} />;
  const thread = ({ row, name, detail }: Thread) => item(row, name, null, <><PersonaTag persona={row.persona} /><Detail detail={detail} /></>);
  const row = (session: AgentSession) => {
    const key = runningKey(session), threads = threadsOf.get(session) ?? [];
    const inHand = [session, ...threads.map(one => one.row)].some(one => runningKey(one) === chat);
    const over = threads.filter(one => one.row.status === "ended");
    // The thread in hand shows even when its fold is closed.
    const overOpen = unfolded.has(key) || over.some(one => runningKey(one.row) === chat);
    const fold = () => setUnfolded(open => new Set(open.has(key) ? [...open].filter(one => one !== key) : [...open, key]));

    // What its threads do shows on the chat: one asking, and how many work.
    const { asking, working } = chatActivity(threads);
    const sub = working ? `${session.machine_name} · ${working} ${working === 1 ? "thread" : "threads"} working` : session.machine_name;
    const right = asking && session.status !== "asking" ? <span className="chip wait">{asking === 1 ? "thread needs you" : `${asking} threads need you`}</span> : null;

    return <Fragment key={key}>
      {item(session, session.title || agentName(session.agent), <HarnessIcon agent={session.agent} />, sub, right)}
      {(!!threads.length || inHand) && <div className="asst-threads">
        {threads.filter(one => one.row.status !== "ended").map(thread)}
        {inHand && session.status !== "ended" && <Button look="quiet" small onClick={() => newThread(session)}>New thread</Button>}
        {!!over.length && <EndedFold count={over.length} open={overOpen} onToggle={fold} />}
        {overOpen && over.map(thread)}
      </div>}
    </Fragment>;
  };

  return <>
    <div className="pj-list asst-list">
      <div className="asst-head"><h4>Chats{!!running.length && <span className="n">{running.length}</span>}</h4><Button small onClick={() => goChat(null)}>New chat</Button></div>
      {running.map(row)}
      {!!ended.length && <EndedFold count={ended.length} open={endedOpen} onToggle={() => setEndedOpen(!endedOpen)} />}
      {endedOpen && ended.map(row)}
    </div>
    <WidthGrip column={CHATS} />
  </>;
}

/// A chat with threads, beside it or one of them in place of the sidebar, as a project's sessions
/// are: Back to the Assistant's chats, New thread, then the chat and its running threads and the
/// ended ones. `current` is the running key of the session on screen.
export function ChatRail({ rail, current }: { rail: NonNullable<ReturnType<typeof chatRail>>; current: string | null }) {
  const { chat, running, past } = rail;

  return <>
    <div className="cc-head" data-tauri-drag-region="" />
    <div className="cc-brand"><Button look="quiet" className="rail-back" title="Back to the Assistant's chats" onClick={() => goChats()}>Assistant</Button>{chat.status !== "ended" && <Button look="quiet" small onClick={() => newThread(chat)}>New thread</Button>}<SideToggle /></div>
    <RailList noun="threads" head={running[0] === chat ? { kind: "agent", ...chat } : null} running={running.filter(row => row !== chat).map(row => ({ kind: "agent", ...row }))} past={past} current={current} more={() => goChats()} />
  </>;
}

/// The Assistant's chats, beside its start composer.
function goChats() {
  go("pane", { assistant: "chats" });
}

function Detail({ detail }: { detail: ThreadDetail }) {
  if (!detail) return null;
  if (detail.kind === "question") return detail.text;
  return <span className="nt-md" dangerouslySetInnerHTML={{ __html: detail.html }} />;
}

/// Starts the message that asks `chat`'s agent for a thread, and opens the chat for the user
/// to say what the thread is for.
export function newThread(chat: AgentRow) {
  void import("./messages.ts").then(({ draftNewThread }) => {
    draftNewThread(chat);
    if (openChat.getState().key !== runningKey(chat)) goChat(chat);
  });
}

/// Opens `session`'s chat on the page, or a new chat.
function goChat(session: AgentRow | null) {
  if (session) go("pane", { agent: session, from: ASSISTANT_CHATS });
  else go("pane", { assistant: "new" });
}

