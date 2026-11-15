import { useEffect, useSyncExternalStore, type ReactNode } from "react";
import { Button, Spinner } from "../ui/kit.tsx";
import type { MachineView } from "./machine-state.ts";
import { agentsLine, wslKey, wslState, type WslActions } from "./wsl-state.ts";

/// The WSL distributions of Windows machine `host`, each with the coding
/// agents found in it, and the action that adds it as a machine of its own or
/// how that machine stands. `machines` names the machines they joined as.
/// Opening it has `host` look into its stopped distributions too.
export function WslRows({ host, machines, actions }: { host: MachineView; machines: readonly MachineView[]; actions: WslActions }) {
  const work = useSyncExternalStore(actions.work.subscribe, actions.work.snapshot);
  const online = host.online;
  useEffect(() => { if (online) actions.probe(host.id); }, [actions, host.id, online]);

  return <div className="wsl-rows">{(host.env ? host.env.wsl : []).map(distro => {
    const key = wslKey(host.id, distro.name), state = wslState(distro, machines), doing = work.get(key) ?? null;
    const button = (label: string, act: (host: string, distro: string) => void) => <Button small look="quiet" disabled={!online} onClick={() => act(host.id, distro.name)}>{label}</Button>;
    const on = button("Turn on", actions.start), off = button("Turn off", actions.stop);
    let right: ReactNode;

    if (doing?.step === "busy") right = <span className="am-wait"><Spinner />{doing.text}</span>;
    else if (state.step === "running") right = <><span className="dmuted">{state.problem ? "Not running" : `Running as ${state.name}`}</span>{state.problem && on}{off}</>;
    else if (state.step === "off") right = <><span className="dmuted">Off</span>{on}</>;
    else if (state.step === "starting") right = <><span className="dmuted">{state.problem ? "Not running" : "Starting"}</span>{state.problem && on}{off}</>;
    else if (doing?.step === "added") right = <span className="dmuted">Added. It shows under Machines shortly.</span>;
    else right = <Button small disabled={!online} title={online ? undefined : `${host.name} is offline`} onClick={() => actions.add(host.id, distro.name)}>Add {distro.name} as a machine</Button>;

    const error = doing?.step === "failed" ? doing.error : state.step === "starting" || state.step === "running" ? state.problem : null;
    return <div key={key} className="wsl-row">
      <div className="am-line"><span className="am-mac-text"><b>{distro.name}</b> <span className="dmuted dmono">{agentsLine(distro)}</span></span>{right}</div>
      {error && <div className="am-msg bad">{error}</div>}
    </div>;
  })}</div>;
}
