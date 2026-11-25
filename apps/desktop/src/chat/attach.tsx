import { agentMessage } from "../agents/session-frame-state.ts";
import { showFieldDrop } from "../agents/composer-ui.tsx";
import { hideDropOverlay, PATH_TYPE, showDropOverlay } from "./drop-overlay.tsx";
import { pastedFiles } from "./pics.ts";
import { paneSession } from "../agentpane/media.ts";
import { addMention, uploadDropped } from "../agents/workdrop.ts";
import { bar, S } from "../bar/core.ts";

/// Whether the agent view's message field is on screen, to take what is
/// pasted or dropped.
const agentField = () => S.view === "agent" && !!agentMessage.current;

document.addEventListener("paste", e => {
  if (!agentField()) return;
  const files = pastedFiles(e);
  if (!files) return;
  e.preventDefault();
  void uploadDropped(files, null, paneSession()?.cwd ?? "", true);
});

/// Files pasted or dropped on a coding agent session upload to its machine
/// and are mentioned; a picture also attaches inline to the message.
export async function agentDrop(files: Iterable<File>) {
  await uploadDropped([...files], null, paneSession()?.cwd ?? "", true);
}

bar.addEventListener("dragover", e => {
  const agent = agentField();
  const types = e.dataTransfer?.types ?? [];

  if (agent && types.includes(PATH_TYPE)) {
    if (!(e.target instanceof Element && e.target.closest("#agconv"))) return;
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = "copy";
    showFieldDrop(true);
    return;
  }

  if (!agent || !types.includes("Files")) return;
  e.preventDefault();
  showDropOverlay(bar);
});
bar.addEventListener("dragleave", e => {
  const into = e.relatedTarget instanceof Element ? e.relatedTarget : null;
  if (!into || !bar.contains(into)) hideDropOverlay();
  if (!into?.closest("#agconv")) showFieldDrop(false);
});
bar.addEventListener("drop", e => {
  hideDropOverlay();
  showFieldDrop(false);
  const path = e.dataTransfer?.getData(PATH_TYPE);

  if (agentField() && path) {
    e.preventDefault();
    addMention(path);
    return;
  }

  if (agentField() && e.dataTransfer?.files.length) {
    e.preventDefault();
    agentDrop(e.dataTransfer.files).then(() => agentMessage.current?.focus());
  }
});
