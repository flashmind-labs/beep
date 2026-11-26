import { togglePreview } from "./text-view.tsx";
// Reading a tab's file from the session's machine, a part at a time, and
// starting a terminal tab.
import { viewKey } from "./view-memory.ts";
import { dropTerm, followTerm, forgetTerm, killTerm, openTerm } from "./terminal.ts";
import { openTabs } from "./media-layout.ts";
import { rememberTerminal } from "./pane-memory.ts";
import { type TreeText, type MediaTab, P, newFileEdit, redraw, refollow } from "./pane-state.ts";
import { closeTab, leavePane, touchTab } from "./pane-tabs.ts";
import { onPhone } from "../ui/platform.ts";
import { resolvePath } from "./media-state.ts";
import { invoke, listen } from "../ui/tauri.ts";
import { paneShown } from "../ui/active.ts";
import { RETRY_MS } from "../agents/reconnect-state.ts";
import { errorText, fileError, isMissing, readError, readFile, tooOld } from "./read-file.ts";

/// Reads the start of a text tab's file.
export async function loadText(t: MediaTab) {
  const s = P.session;
  if (!s || !t.rel) return;

  try {
    t.text = await invoke<TreeText>("agent_tree_text", { machineId: s.machine_id, root: t.root ?? s.cwd, path: t.rel });
    t.size = t.text.size;
    t.tries = 0;
  } catch (e) {
    if (isMissing(errorText(e)) && lookElsewhere(t)) return loadText(t);
    t.error = fileError(e, t.rel);
    readLater(t, e);
  }

  if (openTabs().includes(t)) {
    if (t.editAt && t.text) {
      const at = t.editAt;
      t.editAt = null;
      if (t.text.editable && !onPhone() && !t.edit) t.edit = newFileEdit(t.text.text, at);
    }
    touchTab(t);
    if (t.previewQueued && t.text) {
      t.previewQueued = false;
      if (!t.text.binary) void togglePreview(t);
    }
  }
}

/// Reads `t` again every `RETRY_MS` while its read fails with an error that
/// may pass, as over a link that dropped, and the pane shows. A reconnect
/// reads it at once.
function readLater(t: MediaTab, e: unknown) {
  const text = errorText(e);
  if (isMissing(text) || tooOld(text)) return;
  t.tries = (t.tries ?? 0) + 1;
  readSoon(t);
}

function readSoon(t: MediaTab) {
  clearTimeout(t.retry);
  t.retry = setTimeout(() => { if (paneShown()) readAgain(t); else readSoon(t); }, RETRY_MS);
}

function readAgain(t: MediaTab) {
  clearTimeout(t.retry);
  if (!openTabs().includes(t) || !t.error || !t.tries) return;
  Object.assign(t, { error: "", started: Date.now(), done: 0 });
  touchTab(t);
  if (t.kind === "text") void loadText(t);
  else void load(t);
}

listen("reconnected", () => {
  for (const t of openTabs()) if (t.error && t.tries) readAgain(t);
});

/// Points `t` at the next place the file may be; false when there is
/// nowhere else to look.
function lookElsewhere(t: MediaTab) {
  const there = t.elsewhere?.shift();
  if (!there) return false;

  if (t.kind === "text") {
    t.root = there.root;
    t.rel = there.rel;
  } else {
    t.path = resolvePath(there.root, there.rel);
  }

  refollow();
  return true;
}

/// Starts a terminal tab's shell; when it exits the tab goes.
export async function startTerminal(t: MediaTab) {
  const session = P.session;
  const key = session ? viewKey(session.machine_id, session.id) : null;
  const hooks = {
    onEnd: () => {
      if (t.term) forgetTerm(t.term);
      t.term = undefined;
      if (openTabs().includes(t)) closeTab(t.id);
    },
    onLink: () => drawTermStatus(t),
    onEscape: leavePane,
  };

  try {
    // One that runs already is followed; a new one starts where the tab was opened.
    const term = await (t.existing ? followTerm(t.existing, hooks) : openTerm({ ...t.at!, name: t.name }, hooks));

    if (key) rememberTerminal(key, t, term.row);

    if (!openTabs().includes(t)) {
      // Only an explicit tab close stops a shell that was still starting.
      if (t.closed) {
        dropTerm(term);
        return killTerm(term).catch(e => console.warn("stopping a terminal whose tab closed:", e));
      }

      return dropTerm(term);
    }

    t.term = term;
  } catch (e) {
    if (!openTabs().includes(t)) return;
    t.error = readError(e);
  }

  // Its screen fits the view and takes the keys the pane has.
  touchTab(t);
  redraw();
}

/// A terminal tab's footer says how its terminal stands again, in place of a note.
function drawTermStatus(t: MediaTab) {
  t.note = null;
  touchTab(t);
}

/// Reads a media tab's file. `again` reads it after it changed: what the tab
/// shows stays until the new bytes are here, and stays when they cannot be read.
export async function load(t: MediaTab, again = false) {
  const s = P.session;
  if (!s || !t.path) return;
  if (!again) t.started = Date.now();

  try {
    const { bytes, mime } = await readFile(s.machine_id, t.path, s.id, (done, size) => {
      if (again) return;
      t.done = done;
      t.size = size;
      touchTab(t);
    });
    if (!openTabs().includes(t)) return;
    if (again) replacing(t);
    const was = t.url;
    t.size = bytes.length;
    t.bytes = bytes.buffer as ArrayBuffer;
    t.url = URL.createObjectURL(new Blob([bytes as BlobPart], { type: mime || "application/octet-stream" }));
    if (was?.startsWith("blob:")) URL.revokeObjectURL(was);
    t.tries = 0;
  } catch (e) {
    if (!openTabs().includes(t) || again) return;
    if (isMissing(errorText(e)) && lookElsewhere(t)) return load(t);
    t.error = fileError(e, t.path);
    readLater(t, e);
  }

  // Drawn again now its bytes are here.
  touchTab(t);
}

/// Before a tab takes its file's new bytes: a player goes on from where it
/// stands, playing if it played, and a model is drawn again from them.
function replacing(t: MediaTab) {
  const player = t.player;

  if (player) {
    t.playerAt = player.currentTime;
    if (!player.paused) player.addEventListener("loadedmetadata", () => void player.play().catch(() => {}), { once: true });
  }

  t.model?.dispose();
  t.model = null;
}
