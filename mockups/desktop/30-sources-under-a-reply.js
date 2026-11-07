// ---- Sources under a reply ----
// A reply that used the web gets small chips under it: the site's initial and its domain, pages Beep read first,
// then search results, at most six. Tapping one opens the page (the built-in browser on the phones, the default
// browser on the Mac). Past three the rest fold into "+N", which opens them.
// The reply itself no longer ends with a "Sources:" line.
S.src = "two";
const SRC_PAGES = [
  ["tokyocheapo.com", "Halloween in Tokyo 2026: Parties and Events"],
  ["gaijinpot.com", "Shibuya Halloween: What to Know"],
  ["timeout.com", "The Best Halloween Events in Tokyo"],
  ["japan-guide.com", "Halloween in Japan"],
  ["kawasaki-halloween.com", "Kawasaki Halloween Parade"],
];
const SRC_TINTS = ["var(--m-t1)", "var(--m-t2)", "var(--m-t0)", "#E3ECF7", "#EDE6F7"];
const SRC_ASK = "Halloween events today in Tokyo";
const SRC_REPLY = "Shibuya is the big one, but the city asks people not to gather at the scramble this year, so expect police and closed streets after 18:00. Better picks tonight: the Roppongi Hills costume walk from 17:00, the Ikebukuro Halloween Cosplay Festival around Sunshine City, and a club night in Shinjuku Kabukicho if you want to dance.";
const srcList = () => S.src === "two" ? SRC_PAGES.slice(0, 2) : SRC_PAGES;
// `browser` names where a tap opens it; `dark` is the Mac bar.
function srcChips(browser, dark) {
  const list = srcList(), open = S.src !== "many";
  const shown = open ? list : list.slice(0, 3), more = list.length - shown.length;
  const chip = ([domain, title], i) => `<button class="srcchip${dark ? " dk" : ""}" title="${title}" data-toast="Opens ${domain} in ${browser}">
    <i style="background:${dark ? "var(--d-surface)" : SRC_TINTS[i % SRC_TINTS.length]}">${domain[0].toUpperCase()}</i><span>${domain}</span></button>`;
  return `<div class="srcchips">${shown.map(chip).join("")}${more ? `<button class="srcchip more${dark ? " dk" : ""}" data-srcmore aria-label="Show ${more} more sources">+${more}</button>` : ""}</div>`;
}
{
  const css = document.createElement("style");
  css.textContent = `.srcchips { display: flex; flex-wrap: wrap; gap: 6px; margin-top: -4px }
.srcchip { display: inline-flex; align-items: center; gap: 6px; height: 28px; max-width: 180px; padding: 0 10px 0 4px; border-radius: 999px;
  border: 1.5px solid var(--m-soft); background: var(--m-surface); color: var(--m-ink); font-size: 12px; font-weight: 500 }
.srcchip span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap }
.srcchip i { width: 20px; height: 20px; border-radius: 50%; flex: none; display: grid; place-items: center; font-style: normal; font-size: 10px; font-weight: 600; color: var(--m-ink) }
.srcchip.more { padding: 0 10px; color: var(--m-muted) }
.srcchip:hover { border-color: var(--m-ledge) }
.srcchip.dk { height: 24px; border: 1px solid var(--d-border); background: var(--d-elev); color: var(--d-muted); font-size: 11.5px; padding: 0 9px 0 3px }
.srcchip.dk i { width: 18px; height: 18px; color: var(--d-fg); border: 1px solid var(--d-border) }
.srcchip.dk.more { padding: 0 9px }
.srcchip.dk:hover { color: var(--d-fg); border-color: #34363C }`;
  document.head.appendChild(css);
}
document.addEventListener("click", e => {
  if (!e.target.closest("[data-srcmore]")) return;
  S.src = "open"; render();
});
D.dSources = () => deskScene(dBar(D_ASK + `<div class="dbody">${dYou(SRC_ASK)}
    ${dFold(["Searching the web", "Reading tokyocheapo.com"])}
    <div class="dcoderen agent md">${SRC_REPLY}</div>
    ${srcChips("your browser", true)}
  </div>` + dFollow + dHints()));
VARIANTS.sources = vs("src", [["Two pages", "two"], ["More than three", "many"], ["All shown", "open"]]);
DESK_SCREENS[0].items.splice(DESK_SCREENS[0].items.findIndex(x => x[0] === "dProducts") + 1, 0, ["dSources", "Sources under a reply", "SourceChips"]);
VARIANTS.dSources = VARIANTS.sources;
