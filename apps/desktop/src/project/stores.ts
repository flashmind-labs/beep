// The project page's lists that read without starting agents: terminals, pull requests and
// issues, and a thread read in full.
import { invoke, indexedChannel, openUrl } from "../ui/tauri.ts";
import { formatAgo } from "../ui/format.ts";
import { lazyScreen, makeTerminals } from "../terminals/terminal-state.ts";
import { makeThreads } from "./threads.ts";
import { makeThreadDetail } from "./threaddetail-state.ts";
import { keptSection } from "../center/state.ts";
import { projectChanged } from "./barstate.ts";

export const terms = makeTerminals({ invoke, channel: indexedChannel, makeScreen: () => lazyScreen(() => import("../terminals/termview.ts").then(m => m.makeScreen)), ago: ms => formatAgo(ms, "short"), redraw: () => projectChanged.notify(), kept: keptSection("terminals") });
/// The project's open pull requests and issues, for the project page's list.
export const thr = makeThreads({
  invoke,
  redraw: () => projectChanged.notify(),
  openUrl,
  kept: keptSection("threads"),
});

/// A pull request or issue read in full, in the split's right column.
export const details = makeThreadDetail({ redraw: () => projectChanged.notify() });
