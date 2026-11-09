import { createElement, memo, useState } from "react";
import { $, state } from "./state.ts";
import { showWindowParts } from "./window-root.ts";

export type HomeSubmitResult = { kind: "sent" } | { kind: "failed"; error: string };
interface Actions { send(prompt: string): Promise<HomeSubmitResult>; agent(prompt: string): void }
let actions: Actions | null = null;

export function initHomeComposer(value: Actions) {
  actions = value;
  drawHomeComposer();
}

export function drawHomeComposer() {
  if (!actions) return;
  const machines = state.home?.machines ?? [];
  showWindowParts([{ part: "home-composer", host: $("#home-box"), content: createElement(HomeComposer, {
    visible: state.subscribed, hasMachines: machines.length > 0,
    canStartAgent: machines.some(machine => machine.online && machine.can_host !== false), actions,
  }) }]);
}

const HomeComposer = memo(function HomeComposer({ visible, hasMachines, canStartAgent, actions }: { visible: boolean; hasMachines: boolean; canStartAgent: boolean; actions: Actions }) {
  const [draft, setDraft] = useState(""), [error, setError] = useState("");
  const send = async () => {
    const prompt = draft.trim();
    if (!prompt) return;
    setDraft("");
    setError("");
    const result = await actions.send(prompt);
    if (result.kind === "failed") { setDraft(prompt); setError(result.error); }
  };
  return <form className="cc-comp" id="home-comp" style={{ display: visible ? "" : "none" }} onSubmit={event => { event.preventDefault(); void send(); }}>
    <input id="home-ask" type="text" autoComplete="off" spellCheck={false} aria-label="Ask Beep" placeholder={hasMachines ? "Ask Beep, or start an agent on a machine" : "Ask Beep anything"} value={draft} onChange={event => setDraft(event.currentTarget.value)} />
    <span className="cc-err" id="home-err" role="status">{error}</span>
    <button className="dbtn" type="button" id="home-agent" title="Start a coding agent on a machine" style={{ display: canStartAgent ? "" : "none" }} onClick={() => { const prompt = draft.trim(); setDraft(""); actions.agent(prompt); }}>Agent</button>
    <button className="dbtn pri" type="submit">Send<kbd>&#9166;</kbd></button>
  </form>;
});
