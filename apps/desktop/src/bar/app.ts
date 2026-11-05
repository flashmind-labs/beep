// bar/: the bar's page, its views, keys, hints and live voice, and the pane
// the command center embeds it as. Entry: this file (dist/app.js).
// The bar: the popover the Option double tap opens.
import "../ui/loopclock.ts";
import { followSkin } from "../ui/skin.ts";
import { guardClicks } from "../ui/clickguard.ts";
followSkin();
guardClicks();
import "./events.ts";
import { show } from "./views.ts";

show(null);
