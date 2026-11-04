// The app tile on approval cards: the app's logo, as the Apps page shows
// it, or its first letter only when it has none.
import { type AppLogo, logoFor } from "./approvals.ts";
import { invoke } from "../ui/tauri.ts";
import { esc } from "../ui/format.ts";

/// The apps, listed once per window; a failed list is asked again next time.
let apps: Promise<AppLogo[]> | null = null;

/// An empty tile for `app`, filled in by `fillAppTiles`.
export const appTile = (app: string) => `<span class="dapp" data-app="${esc(app)}"></span>`;

/// Puts each empty tile's logo in, once the apps are listed. A logo that
/// does not load, or an app without one, shows its first letter.
export async function fillAppTiles(root: ParentNode) {
  const tiles = [...root.querySelectorAll<HTMLElement>(".dapp[data-app]")].filter(t => !t.childElementCount && !t.textContent);
  if (!tiles.length) return;
  const list = await (apps ??= invoke<AppLogo[]>("list_apps").catch(() => { apps = null; return []; }));
  for (const tile of tiles) {
    const app = tile.dataset.app ?? "";
    const letter = esc((app || "?")[0].toUpperCase());
    const logo = logoFor(list, app);
    if (!logo) { tile.innerHTML = letter; continue; }
    tile.classList.add("logo");
    tile.innerHTML = `<img src="${esc(logo)}" alt="">`;
    tile.querySelector("img")!.addEventListener("error", () => { tile.classList.remove("logo"); tile.innerHTML = letter; }, { once: true });
  }
}
