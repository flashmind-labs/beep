import { esc, h, invoke } from "./core.ts";
import { sentBox } from "./media.ts";

// ---------- Transit routes ----------

/// A get_route result. The routes live on the server, fetched by the call's
/// id, so their shapes never sit in the model's context.
export const ROUTE_WALK = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="13" cy="4" r="2"/><path d="M15 21l-2-5-3-2 1-5 3 3h3M9 9l-3 3v4"/></svg>';

export const ROUTE_CHEV = '<svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><path d="M9 6l6 6-6 6"/></svg>';

export async function routeRow(turn, callId) {
  const card = h('<div class="dchart droute"><div class="dmuted"><span class="dspin"></span>Loading the route</div></div>');
  card.pick = 0;
  // The rides whose stops are open, as "route:leg".
  card.open = new Set();
  sentBox(turn).append(card);
  try {
    card.route = await invoke("route_card", { callId });
    drawRoute(card);
  } catch {
    card.innerHTML = '<div class="dmuted">Route no longer available</div>';
  }
}

export function routeChanges(r, sep) {
  return r.transfers ? `${sep}${r.transfers} change${r.transfers > 1 ? "s" : ""}` : "";
}

/// A station code as Tokyo's signs draw it, ringed in the line's colour.
export const stationCode = (code, color) => code ? `<span class="rc" style="border-color:${color}">${esc(code)}</span>` : "";

export function drawRoute(card) {
  const routes = card.route.routes, r = routes[card.pick] ?? routes[0];
  if (!r) return void (card.innerHTML = '<div class="dmuted">No route found</div>');
  const leg = (l, i) => {
    if (l.mode === "walk") {
      const km = l.meters >= 1000 ? `, ${(l.meters / 1000).toFixed(1)} km` : "";
      return `<div class="dmuted" style="gap:10px"><span class="rw">${ROUTE_WALK}</span>Walk ${l.minutes} min${km} to ${esc(l.to || "destination")}</div>`;
    }
    const color = l.color || "var(--emerald)", badge = l.line || l.name || l.mode;
    const key = `${card.pick}:${i}`, open = card.open.has(key), between = l.between ?? [];
    const label = `${l.stops} stop${l.stops > 1 ? "s" : ""}`;
    const stops = !l.stops ? "" : between.length
      ? ` <button class="rst m" data-stops="${key}" aria-expanded="${open}">· ${ROUTE_CHEV}${label} ·</button>`
      : ` <span class="m">· ${label} ·</span>`;
    const list = open ? `<span class="rsl">${between.map(s => `<span><span class="n">${esc(s.time)}</span>${stationCode(s.code, color)}${esc(s.name)}</span>`).join("")}</span>` : "";
    return `<div class="rl"><span class="rb" style="background:${color};color:${l.text_color || "var(--ink)"}">${esc(badge)}</span>
      <span class="rt" style="border-color:${color}"><b>${esc(l.name || badge)}${l.headsign ? ` <span class="m">toward ${esc(l.headsign)}</span>` : ""}</b>
      <span><b class="n">${esc(l.departs)}</b> ${stationCode(l.from_code, color)}${esc(l.from)}${l.from_track ? `, platform ${esc(l.from_track)}` : ""}${stops} <b class="n">${esc(l.arrives)}</b> ${stationCode(l.to_code, color)}${esc(l.to || "destination")}</span>${list}</span></div>`;
  };
  const picks = routes.length > 1
    ? `<span class="ranges">${routes.map((o, i) => `<button data-route="${i}" aria-pressed="${i === card.pick}">${o.minutes} min${routeChanges(o, ", ")}</button>`).join("")}</span>` : "";
  card.innerHTML = `<div class="ch"><span class="tt"><b>${esc(`${r.departs} to ${r.arrives} · ${r.minutes} min${routeChanges(r, " · ")}`)}</b>
      <small>${r.live ? "Live times" : "Timetable times, delays not included"}</small></span></div>
    ${picks ? `<div class="foot">${picks}</div>` : ""}
    <div class="rs">${routeSketch(r)}</div>
    <div class="rlegs">${r.legs.map(leg).join("")}</div>
    <div class="foot"><button class="dbtn" data-maps>Open in Maps</button><span class="src">Transitous</span></div>`;
}

export const MAP_TILES = "https://server.arcgisonline.com/ArcGIS/rest/services/Canvas/World_Dark_Gray_Base/MapServer/tile";

/// The route over Esri's dark gray map tiles: walks dotted, rides in their
/// line's colour with a stop at each end. Both are in Web Mercator. The tiles
/// reach past the viewBox, so a wide card has map out to its edges.
export function routeSketch(r) {
  const pts = r.legs.flatMap(l => l.points);
  if (!pts.length) return "";
  const W = 300, H = 132, pad = 14;
  const mx = p => (p.longitude + 180) / 360;
  const my = p => { const s = Math.sin(p.latitude * Math.PI / 180); return 0.5 - Math.log((1 + s) / (1 - s)) / (4 * Math.PI); };
  const xs = pts.map(mx), ys = pts.map(my);
  const left = Math.min(...xs), right = Math.max(...xs), top = Math.min(...ys), bottom = Math.max(...ys);
  // Units per world width, never closer than street level.
  const k = Math.min((W - 2 * pad) / Math.max(right - left, 1e-12), (H - 2 * pad) / Math.max(bottom - top, 1e-12), 256 * 2 ** 17);
  const cx = (left + right) / 2, cy = (top + bottom) / 2;
  const sx = x => W / 2 + (x - cx) * k, sy = y => H / 2 + (y - cy) * k;
  const at = p => [sx(mx(p)).toFixed(1), sy(my(p)).toFixed(1)];
  // A zoom level finer than the scale, so 256 px tiles stay sharp on Retina; the tiles stop at 16.
  const z = Math.max(0, Math.min(16, Math.ceil(Math.log2(k / 256)) + 1)), n = 2 ** z, size = (k / n + 0.5).toFixed(1);
  const world = v => v / k;
  let tiles = "";
  for (let tx = Math.floor((cx + world(-2 * W)) * n); tx <= Math.floor((cx + world(2 * W)) * n); tx++)
    for (let ty = Math.max(0, Math.floor((cy + world(-H)) * n)); ty <= Math.min(n - 1, Math.floor((cy + world(H)) * n)); ty++)
      tiles += `<image href="${MAP_TILES}/${z}/${ty}/${((tx % n) + n) % n}" x="${sx(tx / n).toFixed(1)}" y="${sy(ty / n).toFixed(1)}" width="${size}" height="${size}" preserveAspectRatio="none"/>`;
  const path = ps => ps.map((p, i) => `${i ? "L" : "M"}${at(p).join(",")}`).join("");
  const dot = (p, rad, attrs) => { const [x, y] = at(p); return `<circle cx="${x}" cy="${y}" r="${rad}" ${attrs}/>`; };
  const lines = r.legs.filter(l => l.points.length).map(l => l.mode === "walk"
    ? `<path d="${path(l.points)}" fill="none" stroke="var(--muted)" stroke-width="2.5" stroke-linecap="round" stroke-dasharray="0.1 6"/>`
    : `<path d="${path(l.points)}" fill="none" stroke="${l.color || "var(--emerald)"}" stroke-width="5" stroke-linecap="round" stroke-linejoin="round"/>`).join("");
  const rides = r.legs.filter(l => l.mode !== "walk" && l.points.length);
  const passed = rides.flatMap(l => (l.between ?? []).map(s => dot(s.at, 2.2, `fill="var(--surface)" stroke="${l.color || "var(--emerald)"}" stroke-width="1.4"`))).join("");
  const stops = rides.flatMap(l => [l.points[0], l.points.at(-1)])
    .map(p => dot(p, 4, 'fill="var(--surface)" stroke="var(--fg)" stroke-width="1.8"')).join("");
  return `<svg viewBox="0 0 ${W} ${H}" preserveAspectRatio="xMidYMid meet" role="img" aria-label="Map of the route"><g opacity=".85">${tiles}</g>${lines}${passed}${stops}
    ${dot(pts[0], 5, 'fill="var(--emerald)"')}${dot(pts.at(-1), 6, 'fill="var(--coral)" stroke="var(--surface)" stroke-width="2"')}${routeLabels(rides, p => at(p).map(Number), W, H)}</svg>
    <small>Esri, HERE, Garmin, © OpenStreetMap</small>`;
}

/// A label for each station to get on or off at, with its codes: a change
/// at one station shares one label. A label goes beside its stop, toward the
/// middle, and is left out when no side has room.
export function routeLabels(rides, at, W, H) {
  const labels = [];
  for (const l of rides) {
    const color = l.color || "var(--emerald)";
    for (const [p, name, code] of [[l.points[0], l.from, l.from_code], [l.points.at(-1), l.to, l.to_code]]) {
      const [x, y] = at(p);
      const near = labels.find(b => Math.hypot(b.x - x, b.y - y) < 18);
      if (near) {
        if (code && !near.codes.some(c => c[0] === code)) near.codes.push([code, color]);
        continue;
      }
      if (name) labels.push({ x, y, name, codes: code ? [[code, color]] : [] });
    }
  }
  const placed = [];
  const hits = (a, b) => a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;
  return labels.map(b => {
    const codesW = b.codes.reduce((w, [c]) => w + c.length * 5.6 + 8, 0);
    const w = 8 + codesW + b.name.length * 5.2, h = 16;
    const sides = b.x < W / 2 ? [1, -1] : [-1, 1];
    const tries = sides.flatMap(s => [0, -14, 14].map(dy => ({ x: s > 0 ? b.x + 7 : b.x - 7 - w, y: b.y - h / 2 + dy, w, h })));
    const box = tries.find(t => t.x >= 0 && t.x + w <= W && t.y >= 0 && t.y + h <= H && !placed.some(o => hits(o, t)));
    if (!box) return "";
    placed.push(box);
    let x = box.x + 4;
    const codes = b.codes.map(([c, color]) => {
      const cw = c.length * 5.6 + 4, out = `<rect x="${x.toFixed(1)}" y="${(box.y + 2.5).toFixed(1)}" width="${cw.toFixed(1)}" height="11" rx="2.5" fill="#fff" stroke="${color}" stroke-width="1.5"/><text x="${(x + cw / 2).toFixed(1)}" y="${(box.y + 11).toFixed(1)}" text-anchor="middle" font-size="8" font-weight="700" fill="#10201A">${esc(c)}</text>`;
      x += cw + 4;
      return out;
    }).join("");
    return `<g><rect x="${box.x.toFixed(1)}" y="${box.y.toFixed(1)}" width="${w.toFixed(1)}" height="${h}" rx="4.5" fill="var(--surface)" fill-opacity=".92" stroke="var(--border)"/>${codes}<text x="${x.toFixed(1)}" y="${(box.y + 11.3).toFixed(1)}" font-size="9" font-weight="600" fill="var(--fg)">${esc(b.name)}</text></g>`;
  }).join("");
}

export function routeClick(e) {
  const card = e.target.closest(".droute");
  const stops = e.target.closest("[data-stops]");
  if (stops) {
    const key = stops.dataset.stops;
    if (!card.open.delete(key)) card.open.add(key);
    return drawRoute(card);
  }
  const pick = e.target.closest("[data-route]");
  if (pick) {
    card.pick = +pick.dataset.route;
    return drawRoute(card);
  }
  const r = card.route.routes[card.pick], pts = r.legs.flatMap(l => l.points);
  if (!pts.length) return;
  const at = p => `${p.latitude},${p.longitude}`;
  // Maps plans the trip again between the ends, with its own live times.
  invoke("open_url", { url: `https://maps.apple.com/?saddr=${at(pts[0])}&daddr=${at(pts.at(-1))}&dirflg=r` }).catch(() => {});
}

