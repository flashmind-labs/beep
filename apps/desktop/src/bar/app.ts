import "../agents/copy-markdown.ts";
import { initConversationSwitcher } from "../agents/conversation-switcher.ts";
import { openAgent } from "../agents/session.ts";
import "../ui/contextmenu.ts";
import "../chat/diagrams.ts";
// bar/: the bar's page, its views, keys, hints and live voice, and the pane
// the command center embeds it as. Entry: this file (dist/app.js).
// The bar: the popover the Option double tap opens.
import "../ui/loopclock.ts";
import "../ui/zoom.ts";
import { followSkin } from "../ui/skin.ts";
import { guardClicks } from "../ui/clickguard.ts";
import { followToolUpdates } from "../machines/machines.ts";
followSkin();
guardClicks();
followToolUpdates();
initConversationSwitcher({ open: session => openAgent(session) });
import "./events.ts";
import { show } from "./views.ts";

show(null);
