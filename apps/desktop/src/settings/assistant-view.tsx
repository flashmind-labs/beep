import { useEffect, useRef, useState, type ReactElement } from "react";
import { CODING_AGENTS } from "./settings-nav.ts";
import { SettingRow } from "./overview-view.tsx";
import { Button } from "../ui/kit.tsx";
import { closedPicker, drawPicker, focusSearch, pillContent, type Picked } from "../agents/agentpicker.tsx";
import { loadAssistant, saveAssistant, type AssistantPick } from "./assistant.ts";

/// Which harness, model and thinking level the Home assistant starts with.
export function AssistantView() {
  const [pick, setPick] = useState<AssistantPick | null>(null);
  const [open, setOpen] = useState(false);
  const [picker, setPicker] = useState<ReactElement | null>(null);
  const [error, setError] = useState("");
  const host = useRef<HTMLDivElement>(null);
  const pill = useRef<HTMLButtonElement>(null);

  useEffect(() => { loadAssistant().then(setPick, e => setError(String(e))); }, []);

  useEffect(() => {
    const el = host.current;
    if (!el) return;
    if (!open || !pick) { closedPicker(el); return; }
    const picked: Picked = { agent: pick.agent ?? CODING_AGENTS[0], model: pick.model ?? "", effort: pick.effort, query: "" };
    drawPicker(el, {
      render: setPicker,
      picked,
      offered: CODING_AGENTS,
      everywhere: CODING_AGENTS,
      machine: null,
      noMachine: "Models are listed when you start the assistant.",
      anchor: () => pill.current,
      bounds: () => document.body,
      changed: what => {
        if (what === "models") return;
        const next = { agent: picked.agent, model: picked.model || null, effort: picked.effort };
        setPick(next);
        setError("");
        saveAssistant(next).catch(e => setError(String(e)));
        if (what === "model") setOpen(false);
      },
    });
    focusSearch(el);
    return () => closedPicker(el);
  }, [open, pick]);

  const agent = pick?.agent ?? CODING_AGENTS[0];
  const note = error ? <span className="derr" role="alert">{error}</span> : null;
  return <SettingRow id="assistantSetting" title="Harness" note="The coding agent, model and thinking level the Home assistant starts with" extra={note}>
    <div ref={host}>
      <Button look="quiet" ref={pill} className="agc-pill" aria-expanded={open} disabled={!pick} onClick={() => setOpen(!open)}>{pillContent(agent, pick?.model ?? "", pick?.effort ?? null, null)}</Button>
      {picker}
    </div>
  </SettingRow>;
}
