// "Join with a ticket": this Mac joins the mesh another desktop's Add a machine sheet made a
// ticket for. A Mac alone in its own mesh leaves it and brings its settings along.
import { invoke } from "../ui/tauri.ts";
import { button, el } from "../ui/dom.ts";
import { focusBack, sheetShell } from "../ui/parts.ts";

/// The ticket field, its message and the Join button; `done` runs once this Mac has joined, and
/// `beside` goes next to Join.
export function joinFields(done: () => void, beside: Node) {
  const input = el("input", "am-code");
  input.placeholder = "Paste a ticket";
  input.spellcheck = false;
  input.autocomplete = "off";
  input.setAttribute("aria-label", "Join with a ticket");
  const note = el("div", "am-msg");
  const join = button("Join", "dbtn pri", async () => {
    join.disabled = true;
    note.className = "am-wait";
    note.replaceChildren(el("span", "dspin"), "Joining");
    try {
      await invoke("mesh_join", { ticket: input.value });
      done();
    } catch (e) {
      note.className = "am-msg bad";
      note.textContent = String(e);
      join.disabled = !input.value.trim();
    }
  });
  join.disabled = true;
  input.addEventListener("input", () => { join.disabled = !input.value.trim(); });
  input.addEventListener("keydown", e => { if (e.key === "Enter" && !join.disabled) join.click(); });
  return { input, nodes: [input, note, el("div", "am-line", join, beside)] };
}

let root: HTMLDivElement | null = null, back: Element | null = null, onClose: (() => void) | null = null;

/// Opens the sheet over the window. `options.onClose` runs once it closes.
export function openJoinTicket(options: { onClose?: () => void } = {}) {
  if (root) return;
  onClose = options.onClose || null;
  back = document.activeElement;
  const shell = sheetShell("Join with a ticket", "am-sheet");
  root = shell.root;
  const fields = joinFields(close, button("Cancel", "dbtn quiet", close));
  shell.sheet.append(
    el("div", "am-head", el("b", "", "Join with a ticket"), el("kbd", "", "esc")),
    el("div", "am-os", "Made by Add a machine on another Mac. It works once."),
    ...fields.nodes,
  );
  document.body.append(root);
  document.addEventListener("keydown", onKey, true);
  fields.input.focus();
}

function onKey(e: KeyboardEvent) {
  if (e.key !== "Escape") return;
  e.preventDefault();
  e.stopPropagation();
  close();
}

function close() {
  if (!root) return;
  document.removeEventListener("keydown", onKey, true);
  root.remove();
  root = null;
  focusBack(back);
  const done = onClose;
  onClose = null;
  if (done) done();
}
