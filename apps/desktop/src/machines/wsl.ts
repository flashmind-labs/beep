// The WSL rows' actions, through the app.
import { Channel, invoke } from "../ui/tauri.ts";
import type { WslActions } from "./wsl-state.ts";

export const wslActions: WslActions = {
  add: async (host, distro, step) => {
    const onStep = new Channel<string>();
    onStep.onmessage = step;
    await invoke("add_wsl_machine", { machineId: host, distro, onStep });
  },
  stop: async (host, distro) => {
    await invoke("stop_wsl_machine", { machineId: host, distro });
  },
};
