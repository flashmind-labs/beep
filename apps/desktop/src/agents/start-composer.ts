import { el as h } from "../ui/dom.ts";

const FIELD_MAX = 200;

// Project starts hide the selector because their project is already known.
export function makeStartComposer({ projectPicker = false } = {}) {
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
  const grow = () => {
    if (!input.isConnected) return;
    input.style.height = "auto";
    input.style.height = `${Math.min(input.scrollHeight, FIELD_MAX)}px`;
    input.style.overflowY = input.scrollHeight > FIELD_MAX ? "auto" : "hidden";
  };
  input.addEventListener("input", grow);
  input.addEventListener("keydown", e => {
    if (e.key !== "Enter" || e.shiftKey || e.isComposing || e.defaultPrevented) return;
    e.preventDefault();
    form.requestSubmit();
  });
  return { form, input, project, machine, folder, extra, choices, harness, send, pick, att, err, box, grow };
}
