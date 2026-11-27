import type { LogEntry } from "../../parts/session/agentturns.ts";
import { esc } from "../../ui/format.ts";

/// Consecutive thoughts share a row, keyed by the first entry. Other
/// groups keep their places, including output hidden by a folded turn.
export function groupThoughts(entries: LogEntry[], groups: number[][], offset = 0) {
  const out: number[][] = [];

  for (const group of groups) {
    const previous = out.at(-1);
    const at = group[0];

    if (entries[at - offset].kind === "thought" && previous
      && previous.at(-1)! + 1 === at && entries[previous[0] - offset].kind === "thought") {
      previous.push(...group);
    } else out.push([...group]);
  }

  return out;
}

/// All text stays available when the row opens. A duration is shown only
/// when every thought has one.
export function mergedThought(entries: (LogEntry & { html?: string })[], ats: number[], offset = 0) {
  const thoughts = ats.map(at => entries[at - offset]);
  return {
    kind: "thought",
    text: thoughts.map(en => en.text).join("\n\n"),
    html: thoughts.map(en => en.html ?? `<p>${esc(en.text).replace(/\n/g, "<br>")}</p>`).join("\n"),
    ms: thoughts.every(en => en.ms != null) ? thoughts.reduce((sum, en) => sum + en.ms!, 0) : null,
  };
}
