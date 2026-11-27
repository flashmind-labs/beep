// Leaving an agent view in the command center window: back to the page that opened it.
import { S } from "../../stores/bar/core.ts";
import { asBack, goTo, windowAlone } from "../center/nav.ts";
import type { Location } from "../../stores/center/state.ts";
import { stepOutOfRail, shell } from "../center/window-shell.tsx";
import { isMobile } from "../../ui/platform.ts";
import { setHeroCard, withMove } from "../../ui/motion.ts";
import { AG } from "../../stores/agents/agents-state.ts";
import { runningKey } from "../../stores/agents/running-state.ts";

/// Where a chat's Back goes, and the Assistant page's own on the phone.
export const ASSISTANT_CHATS: Location = { view: "pane", arg: { assistant: "chats" } };

/// Esc from an agent view goes back to the view that opened it. A session that closed leaves at
/// once, with `move` false, so a failed close finds the page it left for. The session shrinks
/// back into its card on the board.
export function goBack(from: Location, move: boolean) {
  if (!move) return void goTo(from);

  const session = AG.page?.session;
  if (session) setHeroCard(runningKey(session));
  withMove("back", () => { goTo(from); });
}

/// Esc in an agent view, before `AG.back`: steps out of a chat's threads rail or leaves the
/// Assistant page for Home. False when `AG.back` should handle it.
export function stepOut(): boolean {
  if (S.view === "agent" && stepOutOfRail()) return true;

  const { assistant } = shell.getState();
  if (!assistant) return false;
  if (windowAlone?.kind === "chat") return true;

  if (isMobile()) {
    asBack(() => { goTo(assistant.picked ? ASSISTANT_CHATS : { view: "home", arg: null }); });
    return true;
  }

  goBack({ view: "home", arg: null }, S.view === "agent");

  return true;
}
