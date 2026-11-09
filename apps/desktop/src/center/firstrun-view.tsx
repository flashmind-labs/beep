import { useLayoutEffect, useRef, useState } from "react";
import { button } from "../ui/dom.ts";

type JoinFields = (done: () => void, beside: Node) => { input: HTMLInputElement; nodes: Node[] };

export function FirstRunScreen({ done, join }: { done: () => void; join: JoinFields }) {
  const [step, setStep] = useState<"choose" | "link">("choose");
  const start = useRef<HTMLButtonElement>(null);

  useLayoutEffect(() => {
    if (step === "choose") start.current?.focus();
  }, [step]);

  return <div className="fr-scrim"><div className="fr-card" role="dialog" aria-label="First run">
    <div className="fr-head">
      <svg width="34" height="34" viewBox="0 0 28 28" fill="currentColor" aria-hidden="true"><path d="M14 1 27 27h-3.1L15.4 10v17h-2.8V10L4.1 27H1Z" /></svg>
      <div><b>{step === "choose" ? "Welcome to Beep" : "Link another desktop"}</b><div className="fr-sub">{step === "choose" ? "Run coding agents on your machines." : "On your other desktop, open Machines, then Add a machine, choose Desktop and copy the ticket. This computer then shares its machines, projects and keys."}</div></div>
    </div>
    {step === "choose" ? <>
      <button ref={start} type="button" className="dbtn fr-btn pri" onClick={done}><span className="fr-big"><b>Start</b><span>Use Beep on this computer. You can add machines later.</span></span></button>
      <button type="button" className="dbtn fr-btn " onClick={() => setStep("link")}><span className="fr-big"><b>Link another desktop</b><span>You already use Beep on another computer. Share its machines, projects and keys with this one.</span></span></button>
    </> : <JoinForm done={done} join={join} back={() => setStep("choose")} />}
  </div></div>;
}

function JoinForm({ done, join, back }: { done: () => void; join: JoinFields; back: () => void }) {
  const host = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const target = host.current;
    if (!target) return;
    const fields = join(done, button("Back", "dbtn quiet", back));
    target.replaceChildren(...fields.nodes);
    fields.input.focus();
    return () => target.replaceChildren();
  }, [done, join, back]);
  return <div ref={host} style={{ display: "contents" }} />;
}
