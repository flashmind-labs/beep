import { HintView, type HintViewModel } from "../bar/hints-view.tsx";
import { useLayoutEffect, useRef, type ReactElement } from "react";

export function GroupHeader(p: { name: string; people: string; avatars: string; info(): void; invite(): void }) {
  return <div className="dhead" id="ghead"><span className="davs" dangerouslySetInnerHTML={{ __html: p.avatars }} /><button className="tt" id="gtitle" onClick={p.info}><b>{p.name}</b><small>{p.people}</small></button><button className="dbtn" id="ginvite" onClick={p.invite}>Invite</button><button className="dbtn" id="ginfo" onClick={p.info}>Info<kbd>⌘I</kbd></button></div>;
}
export function GroupShell(p: { name: string; placeholder: string; people: string; avatars: string; draft: string; message: ReactElement | null; mention: ReactElement | null; hints: HintViewModel; grow(input: HTMLTextAreaElement): void; input(input: HTMLTextAreaElement): void; find(): void; info(): void; invite(): void }) {
  const ask = useRef<HTMLTextAreaElement>(null);
  useLayoutEffect(() => { if (ask.current) p.grow(ask.current); }, []);
  return <><GroupHeader {...p} />
    <div className="dbody pt dgroupbody" id="gbody">{p.message}</div><div id="mention">{p.mention}</div>
    <div className="dinput small"><textarea ref={ask} id="gask" rows={1} placeholder={`Message ${p.placeholder}, @beep to ask Beep`} autoComplete="off" spellCheck defaultValue={p.draft} onInput={event => p.input(event.currentTarget)} onClick={p.find} /></div>
    <HintView value={p.hints} />
  </>;
}
