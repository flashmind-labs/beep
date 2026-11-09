import { Fragment, useEffect, useLayoutEffect, useRef, useState } from "react";
import { AppLogo } from "./applogo-view.tsx";
import { GONE_PIC, loadPic, pictures } from "./media.ts";
import type { GroupApproval, GroupChat, GroupMessage } from "./groups.ts";

interface Props {
  chat: GroupChat | null; error: string; revision: number; pending: { key: number; text: string }[]; approvals: GroupApproval[];
  day(ms: number): string; author(message: GroupMessage): string; color(name: string): string; mentions(text: string): string;
  answer(approval: GroupApproval, allow: boolean): void;
}
function Approval({ approval, answer }: { approval: GroupApproval; answer: Props["answer"] }) {
  const [result, setResult] = useState<"allowed" | "denied" | null>(null);
  if (result) return <div className="dmuted">{result === "allowed" ? "Allowed" : "Denied"}</div>;
  const decide = (allow: boolean) => { setResult(allow ? "allowed" : "denied"); answer(approval, allow); };
  return <div className="dcard"><div className="head"><AppLogo app={approval.app} /><div className="dcol"><b>{approval.action}?</b><small>{approval.app} asks you first</small></div></div>{approval.fields.length > 0 && <div className="kv">{approval.fields.map((field, index) => <Fragment key={index}><span>{field.label}</span><span>{field.value}</span></Fragment>)}</div>}<div className="acts"><button type="button" className="dbtn pri" data-allow="1" onClick={() => decide(true)}>Allow</button><button type="button" className="dbtn warn" data-allow="" onClick={() => decide(false)}>Deny</button></div></div>;
}
function Picture({ id, mime, revision }: { id: string; mime: string; revision: number }) {
  const initial = pictures.get(id);
  const [url, setUrl] = useState<string | null | undefined>(typeof initial === "string" || initial === null ? initial : undefined);
  const reading = useRef(false), mounted = useRef(true);
  useEffect(() => () => { mounted.current = false; }, []);
  useEffect(() => {
    const cached = pictures.get(id);
    if (typeof cached === "string" || cached === null) { if (url !== cached) setUrl(cached); return; }
    if (reading.current) return;
    reading.current = true;
    void loadPic(id, mime).then(value => { reading.current = false; if (mounted.current) setUrl(value); });
  }, [id, mime, revision]);
  return typeof url === "string" ? <img className="dpic" src={url} alt="" /> : url === null ? <span className="dpic gone">{GONE_PIC}</span> : <span className="dpic wait" data-id={id} data-mime={mime} />;
}
function sameMessage(a: GroupMessage, b: GroupMessage) {
  if (a.seq !== b.seq || a.text !== b.text || a.html !== b.html || a.timestamp !== b.timestamp || a.author.kind !== b.author.kind || a.images.length !== b.images.length) return false;
  if (a.author.kind === "member" && b.author.kind === "member" && (a.author.name !== b.author.name || a.author.you !== b.author.you)) return false;
  return a.images.every((image, index) => { const other = b.images[index]; return image.id === other.id && image.mime === other.mime && image.width === other.width && image.height === other.height; });
}
function Message({ message, p }: { message: GroupMessage; p: Props }) {
  const [shown, setShown] = useState(message), node = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    if (sameMessage(shown, message)) return;
    const selected = () => {
      const selection = document.getSelection();
      return !!node.current && !!selection && !selection.isCollapsed && selection.containsNode(node.current, true);
    };
    if (!selected()) { setShown(message); return; }
    const resume = () => { if (!selected()) setShown(message); };
    document.addEventListener("selectionchange", resume);
    return () => document.removeEventListener("selectionchange", resume);
  }, [message, shown]);
  const m = shown, own = m.author.kind === "member" && m.author.you, beep = m.author.kind === "beep", who = p.author(m);
  const content = beep ? m.html : p.mentions(m.text);
  return <div ref={node} className={`dg${own ? " me" : beep ? " beep" : ""}`}>{!own && <span className="who" style={{ color: p.color(who) }}>{who}</span>}{(m.text || !m.images.length) && <div className="b" dangerouslySetInnerHTML={{ __html: content }} />}{m.images.length > 0 && <div className="dpics">{m.images.map(image => <Picture key={image.id} id={image.id} mime={image.mime} revision={p.revision} />)}</div>}</div>;
}
export function GroupMessages(p: Props) {
  if (!p.chat) return p.error ? <div className="derr">{p.error}</div> : null;
  let day = "";
  return <>{p.chat.messages.map(m => { const next = p.day(m.timestamp), show = day !== next; day = next; return <Fragment key={m.seq}>{show && <div className="dday">{next}</div>}<Message message={m} p={p} /></Fragment>; })}
    {p.pending.map(item => <div className="dg me pending" key={item.key}><div className="b" dangerouslySetInnerHTML={{ __html: p.mentions(item.text) }} /></div>)}
    {!p.chat.messages.length && !p.pending.length && <div className="dmuted dblock dnomsg">{"No messages yet.\nSay hi. Beep joins in when it can help."}</div>}
    {p.chat.group.beep_typing && <div className="dmuted"><span className="dtyping"><i /><i /><i /></span>Beep is typing</div>}{p.error && <div className="derr">{p.error}</div>}{p.approvals.map(a => <Approval key={a.query_id} approval={a} answer={p.answer} />)}</>;
}
