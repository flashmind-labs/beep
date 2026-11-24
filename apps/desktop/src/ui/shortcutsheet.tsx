import { useState } from "react";
import { WINDOWS } from "./platform.ts";
import { NO_AUTO, SheetHead } from "./kit.tsx";
import { openSheet } from "./parts.tsx";
import { filterShortcuts, SHORTCUTS, type ShortcutSection } from "./shortcuts.ts";

let shut: (() => void) | null = null;

/// Opens the sheet over the page, or closes it when it is open (⌘/ both ways).
export function toggleShortcuts() {
  if (shut) return shut();
  const { close, render } = openSheet("Keyboard shortcuts", "am-sheet ks-sheet", { onClose: () => { shut = null; } });
  shut = close;
  render(<ShortcutSheet all={SHORTCUTS} />);
}

function ShortcutSheet({ all }: { all: readonly ShortcutSection[] }) {
  const [filter, setFilter] = useState("");
  const found = filterShortcuts(all, filter);
  return <>
    <SheetHead title="Keyboard shortcuts" />
    <input className="ip-field ks-filter" type="text" placeholder="Filter" aria-label="Filter shortcuts"
      {...NO_AUTO} autoFocus value={filter} onChange={event => setFilter(event.currentTarget.value)} />
    <div className="ks-list">{found.length ? found.map(section =>
      <section className="ks-sec" key={section.title}>
        <div className="ks-h">{section.title}</div>
        {section.rows.map(row => <div className="ks-row" key={row.does}>
          <span>{row.does}</span><span className="ks-keys">{caps(WINDOWS ? row.keys.replace("⌥ ⌥", "Ctrl+Alt+Space").replaceAll("⌘", "Ctrl+").replaceAll("⌥", "Alt+").replaceAll("⇧", "Shift+") : row.keys)}</span>
        </div>)}
      </section>) : <p className="am-sub">No shortcut matches.</p>}</div>
  </>;
}

/// Key caps for "⌃← / ⌥←": one cap per key, "or" between alternatives.
function caps(keys: string) {
  return keys.split(" / ").flatMap((alternative, i) => [
    ...(i ? [<i key={`or-${i}`}>or</i>] : []),
    ...alternative.split(" ").map((key, j) => <kbd key={`${i}-${j}`}>{key}</kbd>),
  ]);
}
