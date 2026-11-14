import { useState, type ReactNode } from "react";
import { Button, Spinner } from "../ui/kit.tsx";
import type { MachineView } from "./machine-state.ts";
import { agentsLine, wslKey, wslState, type WslActions, type WslWork } from "./wsl-state.ts";

/// The WSL distributions of Windows machine `host`, each with the coding
/// agents found in it, and the action that adds it as a machine of its own or
/// how that machine stands. `machines` names the machines they joined as.
export function WslRows({ host, machines, actions }: { host: MachineView; machines: readonly MachineView[]; actions: WslActions }) {
  const [work, setWork] = useState<ReadonlyMap<string, WslWork>>(new Map());
  const note = (key: string, next: WslWork | null) => setWork(now => {
    const changed = new Map(now);
    if (next) changed.set(key, next);
    else changed.delete(key);
    return changed;
  });

  const add = async (distro: string) => {
    const key = wslKey(host.id, distro);
    note(key, { step: "busy", text: `Adding ${distro}` });

    try {
      await actions.add(host.id, distro, text => note(key, { step: "busy", text }));
      note(key, { step: "added" });
    } catch (e) {
      note(key, { step: "failed", error: String(e) });
    }
  };

  const stop = async (distro: string) => {
    const key = wslKey(host.id, distro);
    note(key, { step: "busy", text: `Turning off ${distro}` });

    try {
      await actions.stop(host.id, distro);
      note(key, null);
    } catch (e) {
      note(key, { step: "failed", error: String(e) });
    }
  };

  return <div className="wsl-rows">{(host.env ? host.env.wsl : []).map(distro => {
    const key = wslKey(host.id, distro.name), state = wslState(distro, machines), doing = work.get(key) ?? null;
    const off = <Button small look="quiet" disabled={!host.online} onClick={() => { void stop(distro.name); }}>Turn off</Button>;
    let right: ReactNode;

    if (doing?.step === "busy") right = <span className="am-wait"><Spinner />{doing.text}</span>;
    else if (state.step === "joined") right = <><span className="dmuted">Running as {state.name}</span>{state.kept && off}</>;
    else if (state.step === "starting") right = <><span className="dmuted">{state.problem ? "Not running" : "Starting"}</span>{off}</>;
    else if (doing?.step === "added") right = <span className="dmuted">Added. It shows under Machines shortly.</span>;
    else right = <Button small disabled={!host.online} title={host.online ? undefined : `${host.name} is offline`} onClick={() => { void add(distro.name); }}>Add {distro.name} as a machine</Button>;

    const error = doing?.step === "failed" ? doing.error : state.step === "add" ? null : state.problem;
    return <div key={key} className="wsl-row">
      <div className="am-line"><span className="am-mac-text"><b>{distro.name}</b> <span className="dmuted dmono">{agentsLine(distro.agents)}</span></span>{right}</div>
      {error && <div className="am-msg bad">{error}</div>}
    </div>;
  })}</div>;
}
