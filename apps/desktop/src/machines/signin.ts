// The sign-in sheet: a machine of the mesh runs a coding agent's own
// sign-in (sign-in/1) and this Mac shows the link and code it printed, and
// sends back the code the page gives.
import { button, el, present } from "../ui/dom.ts";
import { reason } from "../ui/format.ts";
import { copy, openSheet } from "../ui/parts.ts";
import { invoke } from "../ui/tauri.ts";
import { subscribeChanges, topic } from "../ui/mesh-events.ts";
import { stillRunning, type SignInView } from "./signin-state.ts";

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
  const unfollow = subscribeChanges(topic("accounts"), async () => {
    await step({ step: "state" });
    if (error) throw new Error(error);
  }, machine.id, () => active);
  const shell = openSheet(`Sign in ${tool} on ${machine.name}`, "am-sheet", {
    onClose: () => {
      open = false;
      active = false;
      unfollow();
      // Left before it ended: the machine stops it.
      if (view && stillRunning(view)) void ask(machine.id, tool, { step: "cancel" }).catch(console.warn);
      done();
    },
  });
  const field = el("input", "am-code");
  field.placeholder = "Paste the code";
  field.spellcheck = false;
  field.autocapitalize = "off";
  field.addEventListener("keydown", e => { if (e.key === "Enter") { e.preventDefault(); void send(); } });

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
  async function send() {
    const text = field.value.trim();
    if (!text || sending) return;
    sending = true;
    draw();
    await step({ step: "answer", text });
    field.value = "";
    sending = false;
    draw();
  }

  function draw() {
    if (!active) return;
    const v = view;
    const linkRow = v?.url ? el("div", "am-line",
      el("div", "am-code", v.url),
      button("Copy", "dbtn", e => void copy(v.url!, e.currentTarget as HTMLElement)),
      button("Open in browser", "dbtn pri", () => void invoke("open_url", { url: v.url }).catch(console.warn))) : null;
    const codeRow = v?.code ? el("div", "am-line",
      el("div", "am-code", v.code),
      button("Copy", "dbtn", e => void copy(v.code!, e.currentTarget as HTMLElement))) : null;
    const waiting = v?.state === "waiting";
    const answer = waiting && v.asks_code ? [
      el("div", "am-label", "Code from the page"),
      el("div", "am-line", field, button(sending ? "Sending…" : "Send", "dbtn pri", () => void send())),
      el("div", "am-sub", "Sign in on the page, then paste the code it shows."),
    ] : [];
    const status = !v || v.state === "starting" ? el("div", "am-wait", el("span", "dspin"), `Starting ${tool}'s sign-in on ${machine.name}…`)
      : waiting && !v.asks_code ? el("div", "am-wait", el("span", "dspin"), "Waiting for you to approve it in the browser")
      : v.state === "signed_in" ? el("div", "am-msg", `${tool} is signed in on ${machine.name}.`)
      : v.state === "failed" ? el("div", "am-msg bad", v.reason || `${tool} could not sign in.`)
      : v.state === "idle" ? el("div", "am-msg", "The sign-in was stopped.")
      : null;
    const over = v && !stillRunning(v);
    shell.sheet.replaceChildren(...present([
      el("div", "am-head", el("b", "", `Sign in ${tool} on ${machine.name}`), el("kbd", "", "esc")),
      linkRow && el("div", "am-label", "Link"), linkRow,
      codeRow && el("div", "am-label", "Code to enter there"), codeRow,
      ...answer,
      status,
      error ? el("div", "am-msg bad", error) : null,
      el("div", "gp-foot",
        over && v.state === "failed" ? button("Try again", "dbtn", () => { view = null; void step({ step: "start" }); }) : null,
        button(over ? "Close" : "Cancel", over ? "dbtn pri" : "dbtn quiet", () => shell.close())),
    ]));
    if (waiting && v.asks_code && !sending) field.focus();
  }

  draw();
  void step({ step: "start" });
}
