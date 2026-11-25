import "../agents/copy-markdown.ts";
import "./bar-app.tsx";
import { initConversationSwitcher } from "../agents/conversation-switcher.tsx";
import { openAgent } from "../agents/session.tsx";
import "../chat/diagrams.tsx";
// bar/: the bar's page, its views, keys and hints, and the pane
// the command center embeds it as. Entry: this file (dist/app.js).
// The bar: the popover the Option double tap opens.
import "../ui/window-defaults.ts";
import "../ui/zoom.ts";
import { followSkin } from "../ui/skin.ts";
import { guardClicks } from "../ui/clickguard.ts";
import { followToolUpdates } from "../machines/machines.tsx";
import { perfView } from "../ui/perf.ts";
followSkin();
guardClicks();
followToolUpdates();
perfView("bar");
initConversationSwitcher({ open: session => openAgent(session) });
import "./events.ts";
import { show } from "./views.ts";

show(null);
