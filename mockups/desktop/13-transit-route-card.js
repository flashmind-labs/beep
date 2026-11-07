// ---- Transit route card ----
// get_route answers the model in text; the card fetches the same routes with GetRouteCard { call_id }.
// It lists the legs and draws each line's shape in its own colour over a quiet gray map (Esri tiles on Android and the desktop, Apple Maps on iOS), credited in a corner.
// Where the feed numbers its stations (Tokyo's G10, JY05, A12), the code sits in a badge in the line's colour, on the map and in the legs.
// The map marks every stop ridden through and labels where to get on and off; a change at one station shares one label.
// "6 stops" opens the stops ridden through, with their times.
const ROUTE_OPTS = [
  { dep: "19:01", arr: "19:24", min: 23, tr: 1, live: false, legs: [
    { walk: true, min: 4, to: "Kyobashi", pts: [[6, 78], [16, 70], [22, 66]] },
    { short: "G", name: "Ginza Line", color: "#FF9500", fg: "#0F1011", head: "Asakusa", from: "Kyobashi", fc: "G10", ft: "2", dep: "19:05", to: "Ueno", tc: "G16", arr: "19:16", stops: 6,
      between: [["Nihombashi", "G11", "19:06"], ["Mitsukoshimae", "G12", "19:08"], ["Kanda", "G13", "19:10"], ["Suehirocho", "G14", "19:12"], ["Ueno-hirokoji", "G15", "19:14"]],
      pts: [[22, 66], [34, 58], [44, 46], [52, 40], [58, 30], [64, 24]] },
    { walk: true, min: 3, to: "JR Ueno", pts: [[64, 24], [68, 22]] },
    { short: "JY", name: "Yamanote Line", color: "#80C241", fg: "#0F1011", head: "Ikebukuro", from: "Ueno", fc: "JY05", ft: "2", dep: "19:20", to: "Nippori", tc: "JY07", arr: "19:24", stops: 2,
      between: [["Uguisudani", "JY06", "19:22"]],
      pts: [[68, 22], [80, 16], [92, 12]] },
  ]},
  { dep: "19:03", arr: "19:30", min: 27, tr: 0, live: false, legs: [
    { walk: true, min: 6, to: "Takaracho", pts: [[6, 78], [14, 82], [20, 80]] },
    { short: "A", name: "Asakusa Line", color: "#E85298", fg: "#fff", head: "Oshiage", from: "Takaracho", fc: "A12", ft: "1", dep: "19:10", to: "Asakusa", tc: "A18", arr: "19:22", stops: 6,
      between: [["Nihombashi", "A13", "19:12"], ["Ningyocho", "A14", "19:14"], ["Higashi-nihombashi", "A15", "19:16"], ["Asakusabashi", "A16", "19:18"], ["Kuramae", "A17", "19:20"]],
      pts: [[20, 80], [34, 72], [50, 64], [64, 52], [74, 40]] },
    { walk: true, min: 8, to: "destination", pts: [[74, 40], [84, 26], [92, 12]] },
  ]},
  { dep: "19:01", arr: "19:46", min: 45, tr: 0, live: false, legs: [
    { walk: true, min: 45, to: "destination", meters: 3600, pts: [[6, 78], [30, 70], [52, 52], [70, 36], [92, 12]] },
  ]},
];

// The point a fraction `f` along a polyline, for the mock's stops; the apps get each stop's own coordinates.
function along(pts, f) {
  const seg = pts.slice(1).map((p, i) => Math.hypot(p[0] - pts[i][0], p[1] - pts[i][1]));
  let left = f * seg.reduce((a, b) => a + b, 0);
  for (let i = 0; i < seg.length; i++) {
    if (left <= seg[i] || i === seg.length - 1) {
      const t = seg[i] ? Math.min(1, left / seg[i]) : 0;
      return [pts[i][0] + (pts[i + 1][0] - pts[i][0]) * t, pts[i][1] + (pts[i + 1][1] - pts[i][1]) * t];
    }
    left -= seg[i];
  }
  return pts[pts.length - 1];
}

// A station code as Tokyo signs draw it: a white square ringed in the line's colour.
const codeBadge = (code, color) => code
  ? `<span style="display:inline-grid;place-items:center;min-width:28px;height:17px;padding:0 3px;border-radius:4px;border:2px solid ${color};background:#fff;color:#0F1011;font-size:10px;font-weight:700;line-height:1;font-variant-numeric:tabular-nums;vertical-align:1px">${code}</span>` : "";

// The shape of one route, scaled to the card. Walks are dotted, lines are drawn in their colour,
// each stop ridden through is a small dot, and the stations to get on and off at are labelled with their codes.
function routeSketch(r, dark) {
  const W = 300, H = 136, X = x => x * 3, Y = y => 8 + y * 1.36;
  const path = pts => pts.map(([x, y], i) => `${i ? "L" : "M"}${X(x).toFixed(1)},${Y(y).toFixed(1)}`).join("");
  const ink = dark ? "#8A8F98" : "#5D6168", page = dark ? "#1C1D1F" : "#FFFFFF", fg = dark ? "#E3E4E6" : "#0F1011";
  const rides = r.legs.filter(l => !l.walk);
  const lines = r.legs.map(l => l.walk
    ? `<path d="${path(l.pts)}" fill="none" stroke="${ink}" stroke-width="2.5" stroke-linecap="round" stroke-dasharray="0.1 6"/>`
    : `<path d="${path(l.pts)}" fill="none" stroke="${l.color}" stroke-width="5" stroke-linecap="round" stroke-linejoin="round"/>`).join("");
  const small = rides.flatMap(l => l.between.map((_, i) => along(l.pts, (i + 1) / (l.between.length + 1)))
    .map(([x, y]) => `<circle cx="${X(x).toFixed(1)}" cy="${Y(y).toFixed(1)}" r="2.2" fill="${page}" stroke="${l.color}" stroke-width="1.4"/>`)).join("");
  const ends = rides.flatMap(l => [l.pts[0], l.pts[l.pts.length - 1]])
    .map(([x, y]) => `<circle cx="${X(x).toFixed(1)}" cy="${Y(y).toFixed(1)}" r="4" fill="${page}" stroke="${fg}" stroke-width="1.8"/>`).join("");
  // One label per station: a change shares the label of the stop it arrives at.
  const labels = [];
  rides.forEach(l => [[l.pts[0], l.from, l.fc], [l.pts[l.pts.length - 1], l.to, l.tc]].forEach(([[x, y], name, code]) => {
    const near = labels.find(b => Math.hypot(b.x - X(x), b.y - Y(y)) < 24);
    if (near) return void near.codes.push([code, l.color]);
    labels.push({ x: X(x), y: Y(y), name, codes: [[code, l.color]] });
  }));
  const tags = labels.map(b => {
    const cw = b.codes.filter(c => c[0]).reduce((w, [c]) => w + c.length * 6 + 8, 0), w = cw + b.name.length * 5.4 + 10;
    const left = b.x > W / 2, x0 = left ? b.x - 8 - w : b.x + 8, y0 = Math.min(H - 22, Math.max(4, b.y - 9));
    let cx = x0 + 4;
    const badges = b.codes.filter(c => c[0]).map(([c, col]) => {
      const bw = c.length * 6 + 4, s = `<rect x="${cx}" y="${y0 + 3}" width="${bw}" height="12" rx="2.5" fill="#fff" stroke="${col}" stroke-width="1.6"/><text x="${cx + bw / 2}" y="${y0 + 12.2}" text-anchor="middle" font-size="8.5" font-weight="700" fill="#0F1011">${c}</text>`;
      cx += bw + 4;
      return s;
    }).join("");
    return `<g><rect x="${x0.toFixed(1)}" y="${y0}" width="${w.toFixed(1)}" height="18" rx="5" fill="${page}" fill-opacity=".92" stroke="${dark ? "#222326" : "#2A2C3A"}" stroke-width="1"/>${badges}<text x="${cx}" y="${y0 + 12.5}" font-size="9.5" font-weight="600" fill="${fg}">${b.name}</text></g>`;
  }).join("");
  const [sx, sy] = r.legs[0].pts[0], last = r.legs[r.legs.length - 1].pts, [ex, ey] = last[last.length - 1];
  return `<svg viewBox="0 0 ${W} ${H}" preserveAspectRatio="xMidYMid meet" role="img" aria-label="Map of the route" style="display:block;width:100%;height:100%;font-family:var(--body)">
    ${lines}${small}${ends}<circle cx="${X(sx)}" cy="${Y(sy)}" r="5" fill="${dark ? "#5E6AD2" : "#5E6AD2"}"/><circle cx="${X(ex)}" cy="${Y(ey)}" r="6" fill="#F2994A" stroke="${page}" stroke-width="2"/>${tags}</svg>`;
}
const routeHead = r => `${r.dep} to ${r.arr} · ${r.min} min${r.tr ? ` · ${r.tr} change${r.tr > 1 ? "s" : ""}` : ""}`;
const walkIcon = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="13" cy="4" r="2"/><path d="M15 21l-2-5-3-2 1-5 3 3h3M9 9l-3 3v4"/></svg>';
const stopChev = open => `<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" style="transform:rotate(${open ? 90 : 0}deg);transition:transform .2s"><path d="M9 6l6 6-6 6"/></svg>`;
// The stops ridden through, one line each: time, code and name, muted.
const stopList = (l, muted) => `<span style="display:grid;gap:1px;margin:2px 0 3px">${l.between.map(([n, c, t]) =>
  `<span style="display:flex;align-items:center;gap:6px;color:${muted}"><span style="font-variant-numeric:tabular-nums;min-width:34px">${t}</span>${codeBadge(c, l.color)}${n}</span>`).join("")}</span>`;
document.addEventListener("click", e => {
  const b = e.target.closest('[data-act^="routePick:"], [data-act^="routeStops:"]');
  if (!b) return;
  if (b.dataset.act.startsWith("routeStops:")) S.route = S.route === "stops" ? "routes" : "stops";
  else S.route = b.dataset.act.endsWith(":1") ? "second" : "routes";
  render();
});
