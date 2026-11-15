// The WSL rows' actions, through the app. Their work is kept here, so an
// add goes on and still shows after its sheet or page closes.
import { Channel, invoke } from "../ui/tauri.ts";
import { errorText } from "./machines-state.ts";
import { WslWorkStore, wslKey, type WslActions, type WslWork } from "./wsl-state.ts";

const work = new WslWorkStore();

/// Runs `call` for `distro` on `host`, showing `busy` meanwhile, then
/// `done` or why it failed.
async function run(host: string, distro: string, busy: string, done: WslWork | null, call: (step: (text: string) => void) => Promise<unknown>) {
  const key = wslKey(host, distro);
  work.set(key, { step: "busy", text: busy });

  try {
    await call(text => work.set(key, { step: "busy", text }));
    work.set(key, done);
  } catch (e) {
    work.set(key, { step: "failed", error: errorText(e) });
  }
}

export const wslActions: WslActions = {
  work,
  add: (host, distro) => void run(host, distro, `Adding ${distro}`, { step: "added" }, step => {
    const onStep = new Channel<string>();
    onStep.onmessage = step;
    return invoke("add_wsl_machine", { machineId: host, distro, onStep });
  }),
  stop: (host, distro) => void run(host, distro, `Turning off ${distro}`, null, () => invoke("stop_wsl_machine", { machineId: host, distro })),
  start: (host, distro) => void run(host, distro, `Turning on ${distro}`, null, () => invoke("start_wsl_machine", { machineId: host, distro })),
  probe: host => {
    invoke("probe_wsl_machines", { machineId: host }).catch((e: unknown) => console.warn("looking into the WSL distributions:", e));
  },
};
