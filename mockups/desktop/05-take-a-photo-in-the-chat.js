// ---- Take a photo in the chat ----
// The image button opens a sheet: a live camera tile first, then recent photos. The camera opens inside Beep,
// so a photo goes straight into the composer without a trip through the system camera app.
Object.assign(S, { cam: "sheet", camPick: [] });
// Made-up photos drawn as SVG: a houseplant with brown leaf edges, from slightly different angles.
const plantPic = (k = 0, wall = "#E9E2D6") => `<svg viewBox="0 0 120 160" preserveAspectRatio="xMidYMid slice" style="display:block;width:100%;height:100%">
  <rect width="120" height="160" fill="${wall}"/><rect x="${70 - k * 6}" y="0" width="40" height="70" fill="#F6F1E6" opacity=".8"/>
  <rect y="118" width="120" height="42" fill="#B99A78"/>
  ${[[-38, 0], [-12, 1], [14, 0], [40, 1], [-62, 1]].map(([r, b], i) => `<g transform="translate(${60 + k * 4} 104) rotate(${r + k * 5})">
    <ellipse cx="0" cy="-40" rx="${12 - (i % 2) * 2}" ry="34" fill="#3F7D52"/>${b ? `<ellipse cx="0" cy="-70" rx="${7 - (i % 2)}" ry="6" fill="#9A6A3A"/>` : ""}
    <path d="M0 -6V-72" stroke="#2F6340" stroke-width="1.5"/></g>`).join("")}
  <path d="M${42 + k * 4} 100h36l-5 34h-26z" fill="#C8674E"/><rect x="${40 + k * 4}" y="96" width="40" height="8" rx="2" fill="#D9785F"/></svg>`;
const RECENT = [[0, "#E9E2D6"], [1, "#E4E8E2"], [2, "#E9E2D6"], [0, "#DDE4EA"], [1, "#EDE6DA"], [2, "#E2E6DF"], [0, "#E8E0D4"], [1, "#DFE6E1"]];
