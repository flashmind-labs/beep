import { el as h } from "../ui/dom.ts";
import { esc } from "../ui/format.ts";
import { nextSessionIntro } from "./session-intro.ts";

const FIELD_MAX = 200;

// Project starts hide the selector because their project is already known.
type Presentation = "compact" | "centered";

export function makeStartComposer({ projectPicker = false, presentation = "centered" }: { projectPicker?: boolean; presentation?: Presentation } = {}) {
  const input = h("textarea");
  input.rows = 1;
  input.autocomplete = "off";
  input.spellcheck = false;
  input.setAttribute("aria-label", "Task for a coding agent");
  input.title = "Enter starts, Shift+Enter adds a line";
  const project = h("button", "pill");
  project.type = "button";
  project.title = "The project this agent works on";
  const machine = h("button", "pill");
  machine.type = "button";
  machine.title = "The machine it runs on";
  const folder = h("button", "pill");
  folder.type = "button";
  folder.title = "The checkout, a new worktree, or an existing worktree";
  const extra = h("span", "choices");
  const choices = h("span", "choices", projectPicker && project, machine, folder, extra);
  const harness = h("button", "pill");
  harness.type = "button";
  harness.title = "Harness, model and thinking";
  const send = h("button", "dbtn pri", "Start", h("kbd", "", "⏎"));
  send.type = "submit";
  const pick = h("div", "pick");
  const att = h("div", "ag-att");
  att.hidden = true;
  const err = h("span", "cc-err");
  err.setAttribute("role", "status");
  const row = h("div", "row", choices, h("span", "sp"), harness, send);
  const box = h("div", "box", att, input, row, pick);
  const form = h("form", "cc-pjc", box, err);
  const intro = h("section", "agc-intro");
  let current: Presentation | null = null;
  let busy = false;
  const setBusy = (value: boolean) => {
    busy = value;
    if (current === "centered") {
      send.className = "agc-send";
      send.setAttribute("aria-label", busy ? "Starting" : "Start");
      send.innerHTML = busy ? '<span class="dspin"></span>' : '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M12 19V5M5 12l7-7 7 7"/></svg>';
    } else {
      send.className = "dbtn pri";
      send.replaceChildren(busy ? "Starting" : "Start", h("kbd", "", "⏎"));
    }
  };
  const setPresentation = (to: Presentation) => {
    if (to === current) return;
    current = to;
    const centered = to === "centered";
    form.classList.toggle("centered", centered);
    input.rows = centered ? 3 : 1;
    input.title = centered ? "Command+Enter starts, Shift+Enter adds a line" : "Enter starts, Shift+Enter adds a line";
    if (centered) {
      const welcome = nextSessionIntro();
      intro.innerHTML = `<h3 aria-label="${esc(welcome)}"><span aria-hidden="true">${welcome.split(" ").map((word, i) => `<span class="agc-word" style="--word:${i}">${esc(word)}</span>`).join(" ")}</span></h3><p>A fresh idea. A tricky bug. A place to start.</p>`;
      form.prepend(intro);
      box.after(choices);
    } else {
      intro.remove();
      row.prepend(choices);
    }
    setBusy(busy);
  };
  setPresentation(presentation);
  const grow = () => {
    if (!input.isConnected) return;
    input.style.height = "auto";
    input.style.height = `${Math.min(input.scrollHeight, FIELD_MAX)}px`;
    input.style.overflowY = input.scrollHeight > FIELD_MAX ? "auto" : "hidden";
  };
  input.addEventListener("input", grow);
  input.addEventListener("keydown", e => {
    if (e.key !== "Enter" || e.shiftKey || e.isComposing || e.defaultPrevented) return;
    if (current === "centered" && !e.metaKey && !e.ctrlKey) return;
    e.preventDefault();
    e.stopPropagation();
    form.requestSubmit();
  });
  return { setPresentation, setBusy, centered: () => current === "centered", form, input, project, machine, folder, extra, choices, harness, send, pick, att, err, box, grow };
}
