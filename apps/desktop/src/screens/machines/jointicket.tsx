// "Link another desktop": this computer joins the mesh that issued the ticket.
// A desktop alone in its own mesh leaves it and brings its settings along.
import { flushSync } from "react-dom";
import { invoke } from "../../ui/tauri.ts";
import { openSheet } from "../../ui/parts.tsx";
import { JoinTicketSheet } from "./jointicket-view.tsx";

export const joinTicket = (ticket: string) => invoke("mesh_join", { ticket });

let open: ReturnType<typeof openSheet> | null = null;

/// Opens the sheet over the window. `options.onClose` runs once it closes.
export function openJoinTicket(options: { onClose?: () => void } = {}) {
  if (open) return;
  const sheet = open = openSheet("Link another desktop", "am-sheet", {
    onClose: () => { open = null; options.onClose?.(); },
  });
  flushSync(() => sheet.render(<JoinTicketSheet done={sheet.close} join={joinTicket} />));
}
