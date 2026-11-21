import { useEffect, useRef, useState, type KeyboardEvent, type ReactElement } from "react";
import { invoke } from "../ui/tauri.ts";
import { readAgentHosts, type AgentHost } from "../agents/agents.ts";
import type { MachineView } from "../machines/machine-state.ts";
import { SettingRow } from "./overview-view.tsx";
import { Button } from "../ui/kit.tsx";
import { reason } from "../ui/format.ts";
import { closedPicker, drawPicker, focusSearch, pillContent, type Picked } from "../agents/agentpicker.tsx";
import { connectHermes, disconnectHermes, loadAssistant, onAssistantSaved, saveAssistant, type AssistantPick, type AssistantSetting, type HermesConnection } from "./assistant.ts";
import { assistantAgents, assistantHost, resolveHarness } from "../agents/assistant-state.ts";

/// Where a Hermes dashboard listens unless told otherwise: on its own machine.
const HERMES_HOST = "127.0.0.1";
const HERMES_PORT = "9119";

/// What the Home assistant starts on: Hermes while connected, else a harness, model and thinking
/// level, with the form that connects Hermes.
export function AssistantView() {
  const [setting, setSetting] = useState<AssistantSetting | null>(null);
  const [error, setError] = useState("");

  // Read again after each save, connect and disconnect.
  useEffect(() => {
    const read = () => { loadAssistant().then(setSetting, e => setError(String(e))); };
    read();
    return onAssistantSaved(read);
  }, []);

  if (setting?.hermes) return <HermesRow hermes={setting.hermes} />;

  const pick = (next: AssistantPick) => {
    setSetting({ ...next, hermes: null });
    setError("");
    saveAssistant(next).catch(e => setError(String(e)));
  };
  return <>
    <HarnessRow pick={setting} error={error} picked={pick} />
    <ConnectHermes />
  </>;
}

/// The machine whose models the picker lists for `agent`.
function machineFor(hosts: readonly AgentHost[], own: string | null, agent: string) {
  const host = assistantHost(hosts, own, agent);
  return host ? { id: host.machine_id, name: host.machine_name } : null;
}

/// Which harness, model and thinking level the Home assistant starts with.
function HarnessRow({ pick, error, picked }: { pick: AssistantPick | null; error: string; picked: (next: AssistantPick) => void }) {
  const [open, setOpen] = useState(false);
  const [picker, setPicker] = useState<ReactElement | null>(null);
  const [hosts, setHosts] = useState<readonly AgentHost[]>([]);
  const [own, setOwn] = useState<string | null>(null);
  const [, listed] = useState(0);
  const host = useRef<HTMLDivElement>(null);
  const pill = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    readAgentHosts().then(setHosts, e => console.warn("agent hosts:", e));
    invoke<{ id: string | null }>("mac_control").then(mac => setOwn(mac.id), () => setOwn(null));
  }, []);

  useEffect(() => {
    const el = host.current;
    if (!el) return;
    // What the assistant starts with, so Settings and the start agree.
    const shown = pick && resolveHarness(pick, hosts);
    if (!open || !shown?.agent) { closedPicker(el); return; }
    const offered = assistantAgents(hosts);
    const chosen: Picked = { agent: shown.agent, model: shown.model ?? "", effort: shown.effort, fast: shown.fast ?? null, query: "" };
    drawPicker(el, {
      render: setPicker,
      picked: chosen,
      offered,
      everywhere: offered,
      machine: machineFor(hosts, own, chosen.agent),
      noMachine: "No machine is online to list models.",
      steps: null,
      anchor: () => pill.current,
      changed: what => {
        if (what === "models") return listed(n => n + 1);
        picked({ agent: chosen.agent, model: chosen.model || null, effort: chosen.effort, fast: chosen.agent === "codex" ? chosen.fast : null });
        if (what === "model") setOpen(false);
      },
    });
    focusSearch(el);
    return () => closedPicker(el);
  }, [open, pick, hosts, own]);

  const shown = pick && resolveHarness(pick, hosts);
  const agent = shown?.agent ?? null;
  const note = error ? <span className="derr" role="alert">{error}</span> : null;
  return <SettingRow id="assistantSetting" title="Harness" note="The coding agent, model and thinking level the Home assistant starts with" extra={note}>
    <div ref={host}>
      <Button look="quiet" ref={pill} className="agc-pill" aria-expanded={open} disabled={!agent} onClick={() => setOpen(!open)}>{shown && agent ? pillContent(agent, shown.model ?? "", shown.effort, machineFor(hosts, own, agent)?.id ?? null) : "No agent"}</Button>
      {picker}
    </div>
  </SettingRow>;
}

/// The assistant runs on Hermes: where, and the way back to a harness.
function HermesRow({ hermes }: { hermes: HermesConnection }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  const disconnect = () => {
    setBusy(true);
    setError("");
    disconnectHermes().catch(e => { setError(reason(e)); setBusy(false); });
  };
  const note = `Hermes on ${hermes.machine_name} · ${hermes.host}:${hermes.port} · token …${hermes.last4}`;
  return <SettingRow id="hermesSetting" title="Hermes" note={note} extra={error ? <div className="derr" role="alert">{error}</div> : null}>
    <Button danger disabled={busy} onClick={disconnect}>{busy ? "Disconnecting…" : "Disconnect"}</Button>
  </SettingRow>;
}

/// Connects a Hermes Agent's dashboard, reached from one of the machines, in place of a harness.
function ConnectHermes() {
  const [machines, setMachines] = useState<readonly MachineView[]>([]);
  const [machine, setMachine] = useState("");
  const [host, setHost] = useState(HERMES_HOST);
  const [port, setPort] = useState(HERMES_PORT);
  const [token, setToken] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  // Hermes is reached through a node: a desktop's or a machine's, not a phone's.
  useEffect(() => {
    invoke<MachineView[]>("list_machines").then(list => setMachines(list.filter(m => m.online && m.can_host !== false)), e => console.warn("machines:", e));
  }, []);

  const asking = machines.some(m => m.id === machine) ? machine : machines[0]?.id ?? "";
  const connect = async () => {
    const number = Number(port);
    if (!Number.isInteger(number) || number < 1 || number > 65535) { setError("The port is a number from 1 to 65535."); return; }
    setBusy(true);
    setError("");

    try {
      const check = await connectHermes({ machine: asking, host: host.trim(), port: number, token });
      if (check.state === "refused" || check.state === "unreachable") setError(check.message);
    } catch (e) {
      setError(reason(e));
    } finally {
      setBusy(false);
    }
  };
  const enter = (event: KeyboardEvent) => { if (event.key === "Enter") { event.preventDefault(); void connect(); } };
  const fields = <div className="dline wrap dhermes">
    <select className="hv-in" aria-label="Machine" value={asking} disabled={busy || !machines.length} onChange={e => setMachine(e.currentTarget.value)}>
      {machines.length ? machines.map(m => <option key={m.id} value={m.id}>{m.name}</option>) : <option value="">No machine online</option>}
    </select>
    <input className="hv-in" aria-label="Host" placeholder="Host" autoComplete="off" spellCheck={false} value={host} disabled={busy} onChange={e => setHost(e.currentTarget.value)} onKeyDown={enter} />
    <input className="hv-in port" aria-label="Port" placeholder="Port" inputMode="numeric" autoComplete="off" spellCheck={false} value={port} disabled={busy} onChange={e => setPort(e.currentTarget.value)} onKeyDown={enter} />
    <input className="hv-in" type="password" aria-label="Token" placeholder="Token" autoComplete="off" spellCheck={false} value={token} disabled={busy} onChange={e => setToken(e.currentTarget.value)} onKeyDown={enter} />
  </div>;
  return <SettingRow id="hermesSetting" title="Connect Hermes" note="Runs the assistant on a Hermes Agent instead" extra={<>{fields}{error && <div className="derr" role="alert">{error}</div>}</>}>
    <Button look="primary" disabled={busy || !asking || !host.trim() || !token} onClick={() => void connect()}>{busy ? "Connecting…" : "Connect"}</Button>
  </SettingRow>;
}
