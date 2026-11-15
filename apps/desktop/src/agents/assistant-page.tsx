// The Assistant page: the assistant's chats beside the pane, which shows the
// open chat or the assistant's start composer.
import { useEffect, useState, useSyncExternalStore } from "react";
import { createStore, useStore } from "zustand";
import { AG, type AgentRow } from "./agents.ts";
import { renderAgentNew } from "./agentnew.tsx";
import { assistantRows } from "./assistant-state.ts";
import { agentName, runningKey } from "./running-state.ts";
import { ensureSessions, sessions, sessionsChanged, sessionsRead } from "./sessions.ts";
import { sessionVisible } from "./closing.ts";
import { S } from "../bar/core.ts";
import type { Location } from "../center/state.ts";
import { shell } from "../center/window-shell.tsx";
import { Row, RunningStatus } from "../project/split-view.tsx";
import { Button, EndedFold } from "../ui/kit.tsx";
import { HarnessIcon } from "../ui/icons.tsx";
import { formatAgo } from "../ui/format.ts";

/// What the Assistant page opens on: its chats (the phone shows only the list), or a new chat.
export type AssistantOpen = "chats" | "new";

/// Where the page's own Back and a chat's Back go.
export const ASSISTANT_CHATS: Location = { view: "pane", arg: { assistant: "chats" } };

/// The open chat's key, which its row shows as the current item.
const openChat = createStore<{ key: string | null }>(() => ({ key: null }));

/// The page opened on its list, until the pane shows something else.
let listing = false;
/// The row the pane last opened a chat with, which may not be listed yet.
let lastRow: AgentRow | null = null;

/// Shows the Assistant page with the assistant's start composer.
export function openAssistantPage(open: AssistantOpen) {
  listing = open === "chats";
  renderAgentNew("", ASSISTANT_CHATS, null, true);
  syncAssistantPage();
}

/// The pane opened `row`'s chat, from the page or anywhere else.
export function openedChat(row: AgentRow) {
  lastRow = row;
  listing = false;
}

/// Works out the page from what the pane shows. A chat started from the assistant's composer is
/// on the page before the session list has it.
export function syncAssistantPage() {
  const open = S.view === "agent" ? AG.open : null;

  if (S.view === "agentnew") return show(AG.form.assistant, !listing, null);
  if (!open) return show(false, false, null);

  const id = { machine_id: open.machine_id, id: open.session };
  const same = (row: AgentRow) => row.machine_id === id.machine_id && row.id === id.id;
  const row = sessions.find(same) ?? (lastRow && same(lastRow) ? lastRow : null);
  const on = row ? row.assistant === true && row.reviewer !== true : !!shell.getState().assistant;
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

/// The assistant's chats: New chat, then the running ones, then the ended ones under their fold.
export function AssistantList() {
  const on = useStore(shell, s => !!s.assistant), chat = useStore(openChat, s => s.key);
  useSyncExternalStore(subscribeSessions, () => read);
  const [endedOpen, setEndedOpen] = useState(false);

  // Read again while on screen when the sessions change.
  useEffect(() => {
    if (!on) return;
    const again = () => { void ensureSessions().catch(() => {}); };
    again();
    sessionsChanged.add(again);
    return () => { sessionsChanged.delete(again); };
  }, [on]);

  if (!on) return <div className="pj-list asst-list" hidden />;
  const { running, ended } = assistantRows(sessions.filter(sessionVisible));
  const row = (session: typeof running[number]) => <Row key={runningKey(session)} picked={chat} nav={false} openTile={() => {}} item={{
    key: runningKey(session),
    lead: <HarnessIcon agent={session.agent} />,
    title: session.title || agentName(session.agent),
    sub: session.machine_name,
    right: session.status === "ended" ? formatAgo(session.last_activity, "since") : <RunningStatus row={{ kind: "agent", ...session }} />,
    open: () => goChat(session),
  }} />;

  return <div className="pj-list asst-list">
    <div className="asst-head"><h4>Chats{!!running.length && <span className="n">{running.length}</span>}</h4><Button small onClick={() => goChat(null)}>New chat</Button></div>
    {running.map(row)}
    {!!ended.length && <EndedFold count={ended.length} open={endedOpen} onToggle={() => setEndedOpen(!endedOpen)} />}
    {endedOpen && ended.map(row)}
  </div>;
}

/// Opens `session`'s chat on the page, or a new chat.
function goChat(session: AgentRow | null) {
  // Loaded when used: the window's navigation loads this module.
  void import("../center/views.tsx").then(({ go }) => session
    ? go("pane", { agent: session, from: ASSISTANT_CHATS })
    : go("pane", { assistant: "new" }));
}

