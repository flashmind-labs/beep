// "Not in the mesh yet" on the Machines tab: the account's machines that have not joined, each
// with a button that copies the line to join, minted when it is clicked so each ticket is its own.
import { dbtn as button, copy } from "../ui/parts.ts";
import { invoke } from "../ui/tauri.ts";
import { el as h } from "../ui/dom.ts";
import { machineOs, type MachineCard } from "../center/home-state.ts";
import { dot, line, section, tag } from "../center/pieces.ts";
import { notJoinedSub } from "./notjoined-state.ts";

/// A ticket's line, copied from the button that asked for it; a failure shows on the button.
async function copyJoinLine(on: HTMLButtonElement) {
  on.disabled = true;
  try {
    const made = await invoke<{ command: string }>("mesh_invite", { kind: "Node" });
    await copy(made.command, on);
  } catch (e) {
    console.warn("minting a ticket:", e);
    on.textContent = "Try again";
  }
  on.disabled = false;
}

export function notJoinedSection(machines: MachineCard[]) {
  const rows = machines.map(m => {
    const copyButton: HTMLButtonElement = button("Copy join line", "small", e => { e.stopPropagation(); copyJoinLine(copyButton); });
    return line(dot(m), m.name, notJoinedSub(machineOs(m)), [tag("blocked", "Not joined"), copyButton], () => {});
  });
  return section("sec-notjoined", "Not in the mesh yet", machines.length, null, h("div", "cc-panel", ...rows));
}
