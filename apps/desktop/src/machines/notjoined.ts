// "Not in the mesh yet" on the Machines tab: the account's machines that have not joined, each
// with a button that copies the line to join, minted when it is clicked so each ticket is its own.
import { invoke } from "../ui/tauri.ts";

/// A new ticket's line to join the mesh.
export async function mintJoinLine() {
  const made = await invoke<{ command: string }>("mesh_invite", { kind: "Node" });
  return made.command;
}
