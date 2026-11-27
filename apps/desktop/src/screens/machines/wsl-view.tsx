import type { MachineView } from "../../parts/machines/machine-state.ts";
import { agentsLine } from "../../stores/machines/wsl-state.ts";

/// The WSL distributions of Windows machine `host`, each with the coding
/// agents found in it.
export function WslRows({ host }: { host: MachineView }) {
  return <div className="wsl-rows">{(host.env ? host.env.wsl : []).map(distro => <div key={distro.name} className="am-line">
    <span className="am-mac-text"><b>{distro.name}</b> <span className="dmuted dmono">{agentsLine(distro)}</span></span>
  </div>)}</div>;
}
