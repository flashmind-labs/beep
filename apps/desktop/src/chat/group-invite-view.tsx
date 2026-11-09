import type { MouseEvent } from "react";

export interface GroupInvite { code: string; url: string; qr_svg: string }
export type InviteState = { kind: "loading" } | { kind: "error"; message: string } | { kind: "ready"; invite: GroupInvite; reset: boolean };

export function GroupInviteView({ title, state, copy, reset }: { title: string; state: InviteState; copy: (text: string, button: HTMLElement) => void; reset: () => void }) {
  if (state.kind === "error") return <div className="dwin"><h4>Invite people</h4><p className="derr">{state.message}</p></div>;
  const intro = <p>They scan it with Beep on their phone, or enter the code under New group, Join with a code.</p>;
  if (state.kind === "loading") return <div className="dwin"><h4>Invite people</h4>{intro}<div className="dmuted"><span className="dspin" />Making a code…</div></div>;
  const { invite } = state;
  const copyText = (text: string) => (event: MouseEvent<HTMLButtonElement>) => copy(text, event.currentTarget);
  return <div className="dwin"><h4>Invite to {title}</h4>{intro}
    <div className="qr" dangerouslySetInnerHTML={{ __html: invite.qr_svg }} /><div className="code big">{invite.code}</div>
    <div className="dline wrap dcenter"><button className="dbtn pri" id="icode" onClick={copyText(invite.code)}>Copy code</button><button className="dbtn" id="ilink" onClick={copyText(invite.url)}>Copy link</button><button className="dbtn warn" id="ireset" onClick={reset}>Reset code</button></div>
    <p className="dfine">{state.reset ? "New code made. The old one no longer works. " : ""}Everyone who joins sees the whole chat. Beep never shows one person's memories to the others.</p></div>;
}
