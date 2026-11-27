import { createDocumentScope } from "../../ui/document-root.tsx";
import { ICON } from "../../ui/icons.tsx";

/// The type a row of the tree drags as, its path relative to the session's folder.
export const PATH_TYPE = "application/x-beep-path";

// Dropping images and files on a session attaches them too; a path dragged
// from its Files tab onto its conversation is mentioned.
const dropOverlay = createDocumentScope<"drop">();
let overlaid = false;

/// Covers `host` with the drop hint, once while a drag stays over it.
export function showDropOverlay(host: HTMLElement) {
  if (overlaid) return;
  overlaid = true;
  dropOverlay.set([{ slot: "drop", host, content: <div className="ddrop" id="ddrop"><span style={{ display: "contents" }} dangerouslySetInnerHTML={{ __html: ICON.image }} />Drop images to send, other files to upload</div> }]);
}

/// A drop handled elsewhere, as on the Files tab's tree, ends the overlay.
export const hideDropOverlay = () => { overlaid = false; dropOverlay.remove(["drop"]); };
