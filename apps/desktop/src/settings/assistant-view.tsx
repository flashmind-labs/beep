import { useEffect, useRef, useState, type ReactElement } from "react";
import { CODING_AGENTS } from "./settings-nav.ts";
import { invoke } from "../ui/tauri.ts";
import type { AgentHost } from "../agents/agents.ts";
import { SettingRow } from "./overview-view.tsx";
import { Button } from "../ui/kit.tsx";
import { closedPicker, drawPicker, focusSearch, pillContent, type Picked } from "../agents/agentpicker.tsx";
import { loadAssistant, saveAssistant, type AssistantPick } from "./assistant.ts";

/// The machine whose models the picker lists for `agent`: this one when it offers the harness,
/// else the first online machine that does.
function machineFor(hosts: readonly AgentHost[], own: string | null, agent: string) {
  const offering = hosts.filter(host => host.agents.includes(agent));
  const host = offering.find(h => h.machine_id === own) ?? offering[0];
  return host ? { id: host.machine_id, name: host.machine_name } : null;
}

/// Which harness, model and thinking level the Home assistant starts with.
export function AssistantView() {
  const [pick, setPick] = useState<AssistantPick | null>(null);
  const [open, setOpen] = useState(false);
  const [picker, setPicker] = useState<ReactElement | null>(null);
  const [error, setError] = useState("");
  const [hosts, setHosts] = useState<readonly AgentHost[]>([]);
  const [own, setOwn] = useState<string | null>(null);
  const [, listed] = useState(0);
  const host = useRef<HTMLDivElement>(null);
  const pill = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    invoke<AgentHost[]>("agent_hosts").then(setHosts, e => console.warn("agent hosts:", e));
    invoke<{ id: string | null }>("mac_control").then(mac => setOwn(mac.id), () => setOwn(null));
  }, []);

  useEffect(() => { loadAssistant().then(setPick, e => setError(String(e))); }, []);

  useEffect(() => {
    const el = host.current;
    if (!el) return;
    if (!open || !pick) { closedPicker(el); return; }
    const offered = hosts.length ? [...new Set(hosts.flatMap(host => host.agents))] : CODING_AGENTS;
    const picked: Picked = { agent: pick.agent ?? CODING_AGENTS[0], model: pick.model ?? "", effort: pick.effort, query: "" };
    drawPicker(el, {
      render: setPicker,
      picked,
      offered,
      everywhere: offered,
      machine: machineFor(hosts, own, picked.agent),
      noMachine: "No machine is online to list models.",
      anchor: () => pill.current,
      bounds: () => document.body,
      changed: what => {
        if (what === "models") return listed(n => n + 1);
        const next = { agent: picked.agent, model: picked.model || null, effort: picked.effort };
        setPick(next);
        setError("");
        saveAssistant(next).catch(e => setError(String(e)));
        if (what === "model") setOpen(false);
      },
    });
    focusSearch(el);
    return () => closedPicker(el);
  }, [open, pick, hosts, own]);

  const agent = pick?.agent ?? CODING_AGENTS[0];
  const note = error ? <span className="derr" role="alert">{error}</span> : null;
  return <SettingRow id="assistantSetting" title="Harness" note="The coding agent, model and thinking level the Home assistant starts with" extra={note}>
    <div ref={host}>
      <Button look="quiet" ref={pill} className="agc-pill" aria-expanded={open} disabled={!pick} onClick={() => setOpen(!open)}>{pillContent(agent, pick?.model ?? "", pick?.effort ?? null, machineFor(hosts, own, agent)?.id ?? null)}</Button>
      {picker}
    </div>
  </SettingRow>;
}
