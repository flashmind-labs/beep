// The Keyboard shortcuts sheet: the list in shortcuts.ts, with a filter.
import { S } from "../bar/core.ts";
import { el } from "./dom.ts";
import { openSheet } from "./parts.ts";
import { filterShortcuts, visibleShortcuts, type ShortcutSection } from "./shortcuts.ts";

let shut: (() => void) | null = null;

/// Opens the sheet over the page, or closes it when it is open (⌘/ both ways).
export function toggleShortcuts() {
  if (shut) return shut();
  const all = visibleShortcuts({ server: S.link === "linked", live: S.live });
  const { sheet, close } = openSheet("Keyboard shortcuts", "am-sheet ks-sheet", { onClose: () => { shut = null; } });
  shut = close;
  const filter = el("input", "ip-field ks-filter");
  filter.type = "text";
  filter.placeholder = "Filter";
  filter.setAttribute("aria-label", "Filter shortcuts");
  filter.autocomplete = "off";
  filter.spellcheck = false;
  const list = el("div", "ks-list");
  const draw = () => list.replaceChildren(...sections(filterShortcuts(all, filter.value)));
  filter.addEventListener("input", draw);
  sheet.append(el("div", "am-head", el("b", "", "Keyboard shortcuts"), el("kbd", "", "esc")), filter, list);
  draw();
  filter.focus();
}

function sections(found: readonly ShortcutSection[]) {
  if (!found.length) return [el("p", "am-sub", "No shortcut matches.")];
  return found.map(s => el("section", "ks-sec", el("div", "ks-h", s.title), ...s.rows.map(r =>
    el("div", "ks-row", el("span", "", r.does), el("span", "ks-keys", ...caps(r.keys))))));
}

/// Key caps for "⌃← / ⌥←": one cap per key, "or" between alternatives.
function caps(keys: string) {
  return keys.split(" / ").flatMap((alt, i) => [i ? el("i", "", "or") : null, ...alt.split(" ").map(k => el("kbd", "", k))]);
}
