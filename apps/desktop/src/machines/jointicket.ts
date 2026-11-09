// "Link another desktop": this computer joins the mesh that issued the ticket.
// A desktop alone in its own mesh leaves it and brings its settings along.
import { createElement } from "react";
import { flushSync } from "react-dom";
import { createDocumentView, type DocumentView } from "../ui/document-root.tsx";
import { invoke } from "../ui/tauri.ts";
import { openSheet } from "../ui/parts.ts";
import { JoinTicketSheet } from "./jointicket-view.tsx";

export const joinTicket = (ticket: string) => invoke("mesh_join", { ticket });

let open: ReturnType<typeof openSheet> | null = null;

/// Opens the sheet over the window. `options.onClose` runs once it closes.
export function openJoinTicket(options: { onClose?: () => void } = {}) {
  if (open) return;
  let root: DocumentView | null = null;
  const sheet = open = openSheet("Link another desktop", "am-sheet", {
    onClose: () => { root?.unmount(); root = null; open = null; options.onClose?.(); },
  });
  root = createDocumentView(sheet.sheet);
  flushSync(() => root?.render(createElement(JoinTicketSheet, { done: sheet.close, join: joinTicket })));
}
