import { HintView, type HintViewModel } from "../bar/hints-view.tsx";
import { useLayoutEffect, useRef } from "react";
import { type ChatRow, type ChatsState } from "../bar/core.ts";
import { ICON } from "../ui/icons.tsx";
import { LIVE_CHATS, VOICE_ICON } from "../bar/live.ts";
import { RESEARCHES } from "../research/researches.ts";
import { mark, type ChatAction } from "./chats.ts";
import { formatAgo } from "../ui/format.ts";

interface Props {
  state: ChatsState; error: string; pane: boolean; running: ReadonlySet<string>;
  select(index: number): void; open(row: ChatRow): void; action(action: ChatAction): void;
}
function Rename({ title }: { title: string }) {
  const input = useRef<HTMLInputElement>(null);
  useLayoutEffect(() => { input.current?.focus(); input.current?.select(); }, []);
  return <div className="dchat" aria-selected="true"><input ref={input} id="rename" defaultValue={title} maxLength={200} spellCheck={false} /></div>;
}
function Action({ name, children, run, warn = false }: { name: ChatAction; children: React.ReactNode; run: (action: ChatAction) => void; warn?: boolean }) {
  return <button type="button" className={`dbtn${warn ? " warn" : ""}`} data-a={name} onClick={e => { e.stopPropagation(); run(name); }}>{children}</button>;
}
function Row({ row, index, p }: { row: ChatRow; index: number; p: Props }) {
  const { state } = p, on = index === state.sel, mode = state.mode, actions = on && mode === "actions";
  const run = (action: ChatAction) => { p.select(index); p.action(action); };
  if (on && mode === "rename") return <Rename title={row.title} />;
  if (on && mode === "delete") return <div className="dchat" aria-selected="true"><span className="t"><span>Delete <b>{row.title}</b>?</span></span><span className="acts"><Action name="delete-yes" run={run} warn>Delete<kbd dangerouslySetInnerHTML={{ __html: ICON.ret }} /></Action><Action name="cancel" run={run}>Cancel</Action></span></div>;
  const tail = actions ? <span className="acts">{row.kind === "chat" && <>{!p.pane && <Action name="window" run={run}>Open in window<kbd>⌘O</kbd></Action>}<Action name="rename" run={run}>Rename<kbd>⌘R</kbd></Action></>}<Action name="pin" run={run}>{row.pinned ? "Unpin" : "Pin"}<kbd>⌘P</kbd></Action>{row.kind === "group" ? <><Action name="invite" run={run}>Invite</Action><Action name="info" run={run}>Info</Action><Action name="leave" run={run} warn>Leave</Action></> : <><Action name="share" run={run}>Share</Action><Action name="delete" run={run} warn>Delete<kbd>⌘⌫</kbd></Action></>}</span> : p.running.has(row.id) || row.typing ? <span className="run"><span className="drun" />{row.typing ? "Beep is typing" : "Answering"}</span> : <span className="w">{formatAgo(row.updated_at, "chat")}</span>;
  const content = <>{row.pinned && <span className="pin" aria-label="Pinned" dangerouslySetInnerHTML={{ __html: ICON.pin }} />}{row.kind === "group" && <span className="grp" aria-label="Group" dangerouslySetInnerHTML={{ __html: ICON.group }} />}{RESEARCHES.has(row.id) && <span className="grp" aria-label="Research" dangerouslySetInnerHTML={{ __html: ICON.doc }} />}{LIVE_CHATS.has(row.id) && <span className="grp vmic" aria-label="Spoken" dangerouslySetInnerHTML={{ __html: VOICE_ICON.mic }} />}<span className="t"><span dangerouslySetInnerHTML={{ __html: mark(row.title, state.query) }} />{row.snippet && <small dangerouslySetInnerHTML={{ __html: mark(row.snippet, state.query) }} />}</span>{tail}</>;
  const open = () => { p.select(index); p.open(row); };
  return actions ? <div className="dchat" role="option" aria-selected={on} data-i={index} onClick={open}>{content}</div> : <button type="button" className="dchat" role="option" aria-selected={on} data-i={index} onClick={open}>{content}</button>;
}
function Share({ p }: { p: Props }) {
  const share = p.state.share;
  if (!share || p.state.mode !== "share") return null;
  return <div className="dcard dshare">{share.error ? <div className="derr">{share.error}</div> : !share.url ? <div className="dmuted"><span className="dspin" />Making a link…</div> : <><div className="code">{share.url}</div><div className="dline"><button type="button" className="dbtn pri" data-a="copy-share" onClick={() => p.action("copy-share")}>Copy link</button><button type="button" className="dbtn warn" data-a="unshare" onClick={() => p.action("unshare")}>Stop sharing</button></div></>}<div className="dmuted dblock">A copy of the chat as it is now. Only your messages and Beep's replies are shared, not tool results, thoughts or your location. Anyone with Beep can open the link.</div></div>;
}
export function ChatsView(p: Props) {
  return <>{p.error && <div className="derr dpad">{p.error}</div>}{p.state.list.length ? p.state.list.map((row, index) => <div key={`${row.kind}:${row.id}`} style={{ display: "contents" }}><Row row={row} index={index} p={p} />{index === p.state.sel && <Share p={p} />}</div>) : <div className="dmuted dpad">No chats</div>}</>;
}

export function ChatsShell(p: Props & { hints: HintViewModel; search(value: string): void }) {
  const input = useRef<HTMLInputElement>(null);
  useLayoutEffect(() => { input.current?.focus(); }, []);
  const { list, query } = p.state;
  return <><div className="dinput"><input ref={input} id="find" placeholder="Search chats and groups" autoComplete="off" spellCheck={false} onChange={event => p.search(event.currentTarget.value)} /><kbd>⌘K</kbd></div>
    <div className="dsec" id="sec">{query ? `${list.length} result${list.length === 1 ? "" : "s"}` : "Recent"}</div><div className="dlist" id="list" role="listbox" aria-label="Chats"><ChatsView {...p} /></div>
    <HintView value={p.hints} />
  </>;
}
