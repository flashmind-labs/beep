// "Not in the mesh yet" on the Machines tab: the account's machines that have not joined, each
// with a button that copies the line to join, minted when it is clicked so each ticket is its own.
import { copy } from "../ui/parts.ts";
import { invoke } from "../ui/tauri.ts";

/// A ticket's line, copied from the button that asked for it; a failure shows on the button.
export async function copyJoinLine(on: HTMLButtonElement) {
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

