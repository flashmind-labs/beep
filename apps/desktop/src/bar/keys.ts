import { closeChatFind, openChatFind, stepChatFind } from "../chat/find.tsx";
import { loadOlder } from "../agents/older.ts";
import { logSearchSources, revealLogUnit, watchLogSearch } from "../agents/logwindow.tsx";
import type { ChatBody } from "../ui/turn.ts";
import { commandKey, WINDOWS } from "../ui/platform.ts";
import { closeAgentNewMenus } from "../agents/agentnew.tsx";
import { agentKeys } from "../agents/agents.ts";
import { agentBack } from "../agents/agents-list.ts";
import { agentNewKeys } from "../agents/agentnew.tsx";
import { $, bar, newChat, PANE, S } from "./core.ts";
import { closeFilesMenu } from "../agentpane/files.ts";
import { viewerKeys } from "../chat/media.tsx";
import { newTerminal } from "../agentpane/media.ts";
import { waitingBack, waitingKeys } from "./waiting.ts";
import { windowTakes } from "../ui/windowkeys.ts";
import { invoke } from "../ui/tauri.ts";
import { dialogOpen } from "../ui/parts.tsx";

// ---------- Keys ----------

/// Normalize events dispatched on the document to its body.
const target = (event: KeyboardEvent): Element => event.target instanceof Element ? event.target : document.body;

function back() {
  if (closeAgentNewMenus()) return;
  if (S.view === "agent" || S.view === "agentnew" || S.view === "agents") return agentBack();
  if (S.view === "waiting" && waitingBack()) return;
  if (S.view === "machines") return void import("../settings/settings.tsx").then(m => m.renderSettings());
  if (!PANE) invoke("hide_bar");
}

/// In the window, whether the pane is the view on screen; Home takes the keys
/// while it shows.
let paneMounted = false;
export function mountPane(on: boolean) { paneMounted = on; }
document.addEventListener("keydown", e => {
  const cmd = commandKey(e) && !(WINDOWS && e.target instanceof Element && e.target.closest(".tm-term"));
  if (PANE && !paneMounted) return;
  // A key meant for the sidebar (Enter on a row) is not the pane's.
  const t = target(e);
  if (PANE && !cmd && t !== document.body && !bar.contains(t)) return;
  // The window's palette and pages are the window's.
  if (PANE && cmd && windowTakes(e.key)) return;
  // ⌃` opens a terminal in the session's folder, in its pane.
  if (S.view === "agent" && e.ctrlKey && !cmd && e.key === "`") { e.preventDefault(); return newTerminal(); }
  // Keys typed into a terminal of the pane are the terminal's: Tab, Esc
  // and the arrows go to the shell.
  if (!cmd && t instanceof Element && t.closest(".tm-term")) return;
  if (!e.isComposing && e.key === "Escape" && closeChatFind()) { e.preventDefault(); return; }
  if (cmd && !e.altKey && !e.isComposing && e.key.toLowerCase() === "g" && stepChatFind(e.shiftKey ? -1 : 1)) { e.preventDefault(); return; }

  if (cmd && !e.altKey && !e.shiftKey && !e.isComposing && e.key.toLowerCase() === "f" && S.view === "agent" && !dialogOpen()) {
    const body = $<ChatBody>("#agbody");

    if (body) {
      e.preventDefault();
      const log = () => body.querySelector<HTMLElement>(".dcoderlog");
      openChatFind({
        body,
        sources: () => { const l = log(); return l ? logSearchSources(l) : [{ key: "chat", root: body }]; },
        reveal: key => revealLogUnit(body, key),
        watch: changed => { const l = log(); return l ? watchLogSearch(l, changed) : () => {}; },
        prepare: loadOlder,
      });
      return;
    }
  }

  if (e.key === "Escape" && closeFilesMenu()) { e.preventDefault(); return; }
  if ($("#viewer")) return viewerKeys(e, cmd);
  if (cmd && e.key === "/") { e.preventDefault(); return void import("../ui/shortcutsheet.tsx").then(m => m.toggleShortcuts()); }
  if (e.key === "Escape") { e.preventDefault(); return back(); }
  if (agentNewKeys(e, cmd) || agentKeys(e, cmd) || waitingKeys(e, cmd)) return;
  const key = e.key.toLowerCase();

  if (cmd && e.shiftKey && key === "a") {
    e.preventDefault();
    return newChat();
  }

  if (cmd && e.key === ",") { e.preventDefault(); return void import("../settings/settings.tsx").then(m => m.renderSettings()); }
  if (cmd && key === "q") { e.preventDefault(); return invoke("quit"); }
  // The window picks the agent's start: the project's while one of its sessions shows.
  if (cmd && key === "n") { e.preventDefault(); return newChat(); }
});
