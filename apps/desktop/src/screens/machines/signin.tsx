// The sign-in sheet: a machine of the mesh runs a coding agent's own
// sign-in (sign-in/1) and this Mac shows the link and code it printed, and
// sends back the code the page gives.
import { flushSync } from "react-dom";
import { reason } from "../../ui/format.ts";
import { openSheet } from "../../ui/parts.tsx";
import { invoke } from "../../ui/tauri.ts";
import { subscribeEach, topic } from "../../ui/mesh-events.ts";
import { readSignIn, stillRunning, type SignInView } from "../../stores/machines/signin-state.ts";
import { SignInBody } from "./signin-view.tsx";
import { harnessName } from "../../stores/settings/providers-state.ts";

type Step = { step: "start" } | { step: "state" } | { step: "answer"; text: string } | { step: "cancel" };

const ask = (machineId: string, tool: string, step: Step) => invoke<SignInView>("machine_sign_in", { machineId, tool, step });

let open = false;

/// Opens the sheet for `tool` on `machine` and starts its sign-in; `done`
/// runs once the sheet closes, for a screen the `accounts` push does not reach.
export function openSignIn(machine: { id: string; name: string }, tool: string, done?: () => void) {
  if (open) return;
  open = true;
  let view: SignInView | null = null;
  let error = "";
  let sending = false;
  let active = true;
  let pending: Promise<void> = Promise.resolve();
  // The node pushes how the sign-in stands; an update without it is asked about.
  const unfollow = subscribeEach(topic("sign-in", tool), event => {
    const pushed = readSignIn(event.data);

    if (pushed) {
      error = "";
      view = pushed;
      draw();
      return;
    }

    return step({ step: "state" });
  }, { machine: machine.id, when: () => active });
  const shell = openSheet(`Sign in to ${harnessName(tool)} on ${machine.name}`, "am-sheet", {
    onClose: () => {
      open = false;
      active = false;
      unfollow();
      // Left before it ended: the machine stops it.
      if (view && stillRunning(view)) void ask(machine.id, tool, { step: "cancel" }).catch(console.warn);
      done?.();
    },
  });

  function step(s: Step): Promise<void> {
    pending = pending.then(() => active ? read(s) : undefined);
    return pending;
  }
  async function read(s: Step) {
    try {
      const next = await ask(machine.id, tool, s);

      if (!active) {
        if (s.step === "start" && stillRunning(next)) await ask(machine.id, tool, { step: "cancel" }).catch(console.warn);
        return;
      }

      error = "";
      view = next;
      draw();
    } catch (e) {
      error = reason(e);
    }

    draw();
  }
  async function send(value: string) {
    const text = value.trim();
    if (!text || sending) return;
    sending = true;
    draw();
    await step({ step: "answer", text });
    sending = false;
    draw();
  }

  function draw() {
    if (!active) return;
    flushSync(() => shell.render(<SignInBody
      machineName={machine.name} tool={tool} view={view} error={error} sending={sending} send={send}
      retry={() => { view = null; draw(); void step({ step: "start" }); }}
      close={shell.close}
      openUrl={(url: string) => { void invoke("open_url", { url }).catch(console.warn); }}
    />));
  }

  draw();
  void step({ step: "start" });
}
