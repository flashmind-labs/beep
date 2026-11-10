// The sign-in sheet: a machine of the mesh runs a coding agent's own
// sign-in (sign-in/1) and this Mac shows the link and code it printed, and
// sends back the code the page gives.
import { createElement } from "react";
import { flushSync } from "react-dom";
import { createDocumentView, type DocumentView } from "../ui/document-root.tsx";
import { reason } from "../ui/format.ts";
import { openSheet } from "../ui/parts.ts";
import { invoke } from "../ui/tauri.ts";
import { subscribeChanges, topic } from "../ui/mesh-events.ts";
import { stillRunning, type SignInView } from "./signin-state.ts";
import { SignInBody } from "./signin-view.tsx";

type Step = { step: "start" } | { step: "state" } | { step: "answer"; text: string } | { step: "cancel" };

const ask = (machineId: string, tool: string, step: Step) => invoke<SignInView>("machine_sign_in", { machineId, tool, step });

let open = false;

/// Opens the sheet for `tool` on `machine` and starts its sign-in; `done`
/// runs once the sheet closes, so the row can read the machine again.
export function openSignIn(machine: { id: string; name: string }, tool: string, done: () => void) {
  if (open) return;
  open = true;
  let view: SignInView | null = null;
  let error = "";
  let sending = false;
  let active = true;
  let pending: Promise<void> = Promise.resolve();
  let root: DocumentView | null = null;
  const unfollow = subscribeChanges(topic("sign-in", tool), { deliver: "latest", receive: async () => {
    await step({ step: "state" });
    if (error) throw new Error(error);
  } }, { machine: machine.id, when: () => active });
  const shell = openSheet(`Sign in ${tool} on ${machine.name}`, "am-sheet", {
    onClose: () => {
      open = false;
      active = false;
      unfollow();
      root?.unmount();
      root = null;
      // Left before it ended: the machine stops it.
      if (view && stillRunning(view)) void ask(machine.id, tool, { step: "cancel" }).catch(console.warn);
      done();
    },
  });
  root = createDocumentView(shell.sheet);

  const take = (next: SignInView) => {
    view = next;
    draw();
  };
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
      take(next);
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
    if (!active || !root) return;
    flushSync(() => root?.render(createElement(SignInBody, {
      machineName: machine.name, tool, view, error, sending, send,
      retry: () => { view = null; draw(); void step({ step: "start" }); },
      close: shell.close,
      openUrl: (url: string) => { void invoke("open_url", { url }).catch(console.warn); },
    })));
  }

  draw();
  void step({ step: "start" });
}
