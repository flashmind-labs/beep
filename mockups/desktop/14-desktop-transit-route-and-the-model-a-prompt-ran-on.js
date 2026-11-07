// ---- Desktop: transit route, and the model a prompt ran on ----
// Admins see the model and thinking under every prompt: Jev's pick under Auto, else the chosen model.
// Reopened chats show it too; a turn from before Auto shows the model alone. Everyone else sees
// only the thinking Jev picked.
D.dRoute = () => {
  const st = S.d.route || "routes", pick = st === "second" ? 1 : 0, r = ROUTE_OPTS[pick];
  const open = i => st === "stops" && i === 1;
  const leg = (l, i) => l.walk
    ? `<div class="dmuted" style="gap:10px"><span style="width:30px;display:grid;place-items:center">${walkIcon}</span>Walk ${l.min} min to ${l.to}</div>`
    : `<div style="display:flex;gap:10px;align-items:flex-start;font-size:13px;line-height:19px">
        <span style="min-width:30px;height:20px;border-radius:5px;background:${l.color};color:${l.fg};display:grid;place-items:center;font-size:11px;font-weight:700">${l.short}</span>
        <span style="display:grid;gap:2px;border-left:3px solid ${l.color};padding-left:10px;margin-left:-4px"><b>${l.name} <span style="color:var(--d-muted);font-weight:400">toward ${l.head}</span></b>
          <span style="display:flex;align-items:center;gap:6px;flex-wrap:wrap"><b style="font-variant-numeric:tabular-nums">${l.dep}</b>${codeBadge(l.fc, l.color)}${l.from}, platform ${l.ft}
            <button data-dact="routeStops" aria-expanded="${open(i)}" style="all:unset;cursor:pointer;display:inline-flex;align-items:center;gap:3px;color:var(--d-muted)">· ${stopChev(open(i))}${l.stops} stops ·</button>
            <b style="font-variant-numeric:tabular-nums">${l.arr}</b>${codeBadge(l.tc, l.color)}${l.to}</span>
          ${open(i) ? stopList(l, "var(--d-muted)") : ""}</span></div>`;
  const card = `<div class="dchart" style="gap:10px">
      <div class="ch"><span class="tt"><b>${routeHead(r)}</b><small>Timetable times, delays not included</small></span>
        <span class="ranges">${ROUTE_OPTS.slice(0, 2).map((o, i) => `<button aria-pressed="${i === pick}" data-dact="route:${i}">${o.min} min</button>`).join("")}</span></div>
      <div class="rmap dark" style="height:150px;border-radius:8px;padding:4px">${routeSketch(r, true)}<small>Esri, HERE, Garmin, © OpenStreetMap</small></div>
      <div style="display:grid;gap:8px">${r.legs.map(leg).join("")}</div>
      <div class="foot"><button class="dbtn" data-toast="Opening Apple Maps">Open in Maps</button><span class="src">Transitous</span></div></div>`;
  const ran = st === "manual" ? "Haiku 5.5 · low" : st === "old" ? "Haiku 5.5" : st === "user" ? "medium" : "Haiku 5.5 · medium";
  return deskScene(dBar(D_ASK + `<div class="dbody">${dLog(`
      ${dYou("How do I get to Nippori station?")}<div class="drerunof">${ran}</div>
      ${dHead(dWorked(4, 1), { act: `dset:rsteps:${S.d.rsteps ? "" : 1}`, open: !!S.d.rsteps })}${S.d.rsteps ? dRows(dTool("Planning route: Kyobashi to Nippori")) : ""}
      <div class="dcoderen agent md">Take the <b>Ginza Line</b> from Kyobashi (G10) toward Asakusa to Ueno (G16), then two stops on the <b>Yamanote Line</b> toward Ikebukuro to Nippori (JY07). About 23 minutes.</div>
      ${card}`)}</div>` + dFollow + dHints()));
};
VARIANTS.dRoute = [
  ["Auto picked", () => { S.d.route = "routes"; render(); }, () => (S.d.route || "routes") === "routes"],
  ["Second route", () => { S.d.route = "second"; render(); }, () => S.d.route === "second"],
  ["Stops open", () => { S.d.route = "stops"; render(); }, () => S.d.route === "stops"],
  ["Chosen model", () => { S.d.route = "manual"; render(); }, () => S.d.route === "manual"],
  ["Not admin", () => { S.d.route = "user"; render(); }, () => S.d.route === "user"],
  ["Older turn", () => { S.d.route = "old"; render(); }, () => S.d.route === "old"],
];
DESK_SCREENS[0].items.push(["dRoute", "Transit route", "RouteCard, model line"]);
document.addEventListener("click", e => {
  const b = e.target.closest('[data-dact^="route:"], [data-dact="routeStops"]');
  if (!b) return;
  if (b.dataset.dact === "routeStops") S.d.route = S.d.route === "stops" ? "routes" : "stops";
  else S.d.route = b.dataset.dact.endsWith(":1") ? "second" : "routes";
  render();
}, true);
