import "../../stores/agents/copy-markdown.ts";
import "./bar-app.tsx";
import "../center/screens.ts";
import { initConversationSwitcher } from "../../parts/session/conversation-switcher.tsx";
import { openSession } from "../../parts/session/open-state.ts";
import "../../parts/session/diagrams.tsx";
// bar/: the bar's page, its views, keys and hints, and the pane
// the command center embeds it as. Entry: this file (dist/app.js).
// The bar: the popover the Option double tap opens.
import "../../ui/window-defaults.ts";
import "../../ui/zoom.ts";
import { followSkin } from "../../ui/skin.ts";
import { guardClicks } from "../../ui/clickguard.ts";
import { followToolUpdates } from "../../screens/machines/machines.tsx";
import { perfView } from "../../ui/perf.ts";
followSkin();
guardClicks();
followToolUpdates();
perfView("bar");
initConversationSwitcher({ open: session => openSession(session) });
import "./events.ts";
import { show } from "./views.ts";
import { PANE, windowRequests } from "../../stores/bar/core.ts";
import { openChatSkills } from "../../screens/project/project-skills.tsx";

// The bar has no window to follow requests: the session menu's Skills row opens its sheet here.
if (!PANE) {
  windowRequests.follow(request => {
    if (request.kind === "chat-skills") void openChatSkills(request.folder, request.title);
  });
}

show(null);
