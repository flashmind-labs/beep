// "Link another desktop": this computer joins the mesh that issued the ticket.
// A desktop alone in its own mesh leaves it and brings its settings along.
import { invoke } from "../ui/tauri.ts";
import { button, el } from "../ui/dom.ts";
import { openSheet } from "../ui/parts.ts";

/// The ticket field, its message and the Join button; `done` runs once this Mac has joined, and
/// `beside` goes next to Join.
export function joinFields(done: () => void, beside: Node) {
  const input = el("input", "am-code");
  input.placeholder = "Paste a ticket";
  input.spellcheck = false;
  input.autocomplete = "off";
  input.setAttribute("aria-label", "Desktop ticket");
  const note = el("div", "am-msg");
  const join = button("Link", "dbtn pri", async () => {
    join.disabled = true;
    note.className = "am-wait";
    note.replaceChildren(el("span", "dspin"), "Linking");
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

let open: ReturnType<typeof openSheet> | null = null;

/// Opens the sheet over the window. `options.onClose` runs once it closes.
export function openJoinTicket(options: { onClose?: () => void } = {}) {
  if (open) return;
  const sheet = open = openSheet("Link another desktop", "am-sheet", {
    onClose: () => { open = null; options.onClose?.(); },
  });
  const close = () => sheet.close();
  const fields = joinFields(close, button("Cancel", "dbtn quiet", close));
  sheet.sheet.append(
    el("div", "am-head", el("b", "", "Link another desktop"), el("kbd", "", "esc")),
    el("div", "am-os", "On the other desktop, open Machines, then Add a machine, choose Desktop and copy the ticket. Paste it here to share its machines, projects and keys."),
    ...fields.nodes,
  );
  fields.input.focus();
}
