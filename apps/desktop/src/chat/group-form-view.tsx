import { useLayoutEffect, useRef } from "react";
import { ICON } from "../ui/icons.ts";
import type { GroupFormMode } from "./groups.ts";

interface Props {
  mode: GroupFormMode; name: string; member: string; error: string;
  preview: { name: string; members: string[] } | null;
  avatars: string; hints: string; changeMode(mode: GroupFormMode): void; cancel(): void; go(): void;
}
export function GroupFormView(p: Props) {
  const first = useRef<HTMLInputElement>(null);
  useLayoutEffect(() => { first.current?.focus(); }, [p.mode]);
  const heading = p.mode === "new" ? "New group" : p.mode === "code" ? "Join a group" : `Join ${p.preview?.name ?? ""}?`;
  const subtitle = p.mode === "new" ? "Chat with people, and Beep helps when it can" : p.mode === "code" ? "Enter the code someone sent you" : p.preview?.members.length ? `${p.preview.members.join(", ")} and Beep are in it` : "Beep is in it";
  const icon = p.mode === "join" ? p.avatars : ICON.group;
  return <><div className="dhead"><span className={p.mode === "join" ? "davs" : "dlogo"} style={p.mode === "join" ? undefined : { background: "var(--accent)" }} dangerouslySetInnerHTML={{ __html: icon }} /><span className="tt"><b>{heading}</b><small>{subtitle}</small></span></div>
    <div className="dform">{p.mode === "new" ? <>
      <label className="dfield">Group name<input ref={first} id="jname" maxLength={80} autoComplete="off" spellCheck={false} defaultValue={p.name} /></label>
      <label className="dfield">Your name in the group<input id="jme" maxLength={40} autoComplete="off" spellCheck={false} defaultValue={p.member} /></label>
      <div className="dmuted dblock">You get a code to send to people. Anyone with Beep who enters it can join.</div>
      {p.error && <div className="derr">{p.error}</div>}
      <div className="row"><button className="dbtn" id="jcode" onClick={() => p.changeMode("code")}>Join with a code</button><span className="dfill" /><button className="dbtn quiet" id="jcancel" onClick={p.cancel}>Cancel</button><button className="dbtn pri" id="jgo" onClick={p.go}>Create<kbd dangerouslySetInnerHTML={{ __html: ICON.ret }} /></button></div>
    </> : p.mode === "code" ? <>
      <label className={`dfield${p.error ? " err" : ""}`}>Invite code<input ref={first} id="jtoken" className="mono" maxLength={80} autoComplete="off" spellCheck={false} placeholder="K7QM4-X9TRP" /><span className="note">{p.error || "Ten letters and digits, like K7QM4-X9TRP. A beep://group link works too."}</span></label>
      <div className="row"><button className="dbtn quiet" id="jcancel" onClick={() => p.changeMode("new")}>Cancel</button><button className="dbtn pri" id="jgo" onClick={p.go}>Find group<kbd dangerouslySetInnerHTML={{ __html: ICON.ret }} /></button></div>
    </> : <>
      <div className="dinfo dmut"><div>Beep can use your memories to help the group. It never shows or quotes them to others.</div><div>When Beep answers you, it runs with your apps and your account, never someone else's.</div><div>You get a notification for each new message.</div></div>
      <label className={`dfield${p.error ? " err" : ""}`}>Your name in the group<input ref={first} id="jme" maxLength={40} autoComplete="off" spellCheck={false} defaultValue={p.member} /><span className="note">{p.error || "The others see this name next to your messages"}</span></label>
      <div className="row"><button className="dbtn" id="jcancel" onClick={p.cancel}>Not now</button><button className="dbtn pri" id="jgo" onClick={p.go}>Join group<kbd dangerouslySetInnerHTML={{ __html: ICON.ret }} /></button></div>
    </>}</div><span style={{ display: "contents" }} dangerouslySetInnerHTML={{ __html: p.hints }} /></>;
}
