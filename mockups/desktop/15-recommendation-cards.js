// ---- Recommendation cards ----
// Beep calls show_cards with a kind for each card. Every card has the shared base: picture, title, subtitle,
// up to three fact chips, rating, note and link. A kind the app knows adds its own parts: a place shows open
// now, distance and Directions; media shows a poster and where to watch; an event shows its date and Add to
// calendar. A kind the app does not know yet draws the base alone. Products are kind "product" (see Product cards).
Object.assign(S, { rec: "places" });
const foodPic = (plate, bg) => `<svg viewBox="0 0 160 120" preserveAspectRatio="xMidYMid slice" style="display:block;width:100%;height:100%">
  <rect width="160" height="120" fill="${bg}"/><ellipse cx="80" cy="66" rx="52" ry="36" fill="#F7F3EC"/><ellipse cx="80" cy="64" rx="38" ry="25" fill="${plate}"/>
  <circle cx="68" cy="58" r="7" fill="#fff" opacity=".5"/><circle cx="92" cy="68" r="5" fill="#3A5A2C" opacity=".7"/><rect x="128" y="22" width="5" height="72" rx="2" fill="#B9B2A6"/></svg>`;
const posterPic = (a, b, t) => `<svg viewBox="0 0 80 120" preserveAspectRatio="xMidYMid slice" style="display:block;width:100%;height:100%">
  <defs><linearGradient id="pg${t}" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="${a}"/><stop offset="1" stop-color="${b}"/></linearGradient></defs>
  <rect width="80" height="120" fill="url(#pg${t})"/><circle cx="56" cy="34" r="14" fill="#fff" opacity=".25"/><path d="M0 92l22-18 18 12 20-22 20 18v38H0z" fill="#000" opacity=".3"/></svg>`;
const eventPic = (bg) => `<svg viewBox="0 0 160 120" preserveAspectRatio="xMidYMid slice" style="display:block;width:100%;height:100%">
  <rect width="160" height="120" fill="${bg}"/><path d="M0 120V96c30-10 50-6 80-14s50 4 80-4v42z" fill="#000" opacity=".35"/>
  <path d="M60 0l-30 120M100 0l30 120" stroke="#fff" stroke-width="10" opacity=".12"/><circle cx="80" cy="44" r="10" fill="#FFE9A8"/></svg>`;
const RECS = {
  places: { ask: "Somewhere for dinner tonight near Gràcia, not too pricey", chip: ["P", "Searched places"],
    say: "Three open tonight within 15 minutes' walk. I'd go to La Pubilla: it's the closest and takes bookings.",
    cards: [
      { kind: "place", title: "La Pubilla", sub: "Catalan · Plaça de la Llibertat", facts: ["€€", "650 m"], open: ["Open until 23:00", true], rating: "4.6", reviews: "2,310", note: "Closest, takes bookings", pic: foodPic("#C8674E", "#E9DFD2") },
      { kind: "place", title: "Bar Bodega Quimet", sub: "Tapas · Carrer del Vic", facts: ["€", "900 m"], open: ["Open until 00:30", true], rating: "4.5", reviews: "1,870", note: "Cheapest, no bookings", pic: foodPic("#B7913E", "#DCE3D3") },
      { kind: "place", title: "Pepa", sub: "Wine bar · Carrer de Sèneca", facts: ["€€€", "1.2 km"], open: ["Opens at 20:00", false], rating: "4.7", reviews: "980", note: "Best wine list", pic: foodPic("#6D3B47", "#E4D9E0") },
    ] },
  media: { ask: "A film for tonight, something like Past Lives", chip: ["W", "Searched the web"],
    say: "Three quiet ones in the same vein. Aftersun is the closest in feel, and it's on Filmin.",
    cards: [
      { kind: "media", title: "Aftersun", sub: "2022 · Charlotte Wells", facts: ["Drama", "1h 42m"], watch: "Filmin", rating: "4.1", reviews: "Letterboxd", note: "Closest in feel", pic: posterPic("#E8A07A", "#3B6E8F", 1) },
      { kind: "media", title: "Columbus", sub: "2017 · Kogonada", facts: ["Drama", "1h 44m"], watch: "MUBI", rating: "3.9", reviews: "Letterboxd", note: "Slow and calm", pic: posterPic("#B9C7A4", "#2F5D4A", 2) },
      { kind: "media", title: "Before Sunrise", sub: "1995 · Richard Linklater", facts: ["Romance", "1h 41m"], watch: "Rent on Apple TV", rating: "4.2", reviews: "Letterboxd", note: "One night, two people", pic: posterPic("#F2C46D", "#8A4B5C", 3) },
    ] },
  events: { ask: "Anything good on this weekend? I like live jazz", chip: ["W", "Searched the web"],
    say: "Two jazz nights and an afternoon session. The Jamboree one on Saturday still has tickets.",
    cards: [
      { kind: "event", title: "Marco Mezquida Trio", sub: "Jamboree · Plaça Reial", facts: ["€18", "Jazz"], when: ["SAT", "18", "21:00"], note: "Tickets left", pic: eventPic("#2E3F6B") },
      { kind: "event", title: "Jam session", sub: "Harlem Jazz Club", facts: ["Free", "Jazz"], when: ["FRI", "17", "22:30"], note: "Open jam, no booking", pic: eventPic("#6B2E3F") },
      { kind: "event", title: "Vermut i jazz", sub: "Antiga Fàbrica Damm", facts: ["€12", "Outdoor"], when: ["SUN", "19", "12:00"], note: "Afternoon, with food", pic: eventPic("#2F6B4E") },
    ] },
  base: { ask: "Good podcasts about design?", chip: ["W", "Searched the web"],
    say: "Three that are still running. 99% Invisible is the one to start with.",
    cards: [
      { kind: "podcast", title: "99% Invisible", sub: "Roman Mars", facts: ["Weekly", "~40 min"], rating: "4.8", reviews: "41k", note: "Start here", pic: eventPic("#8A6FD1") },
      { kind: "podcast", title: "Design Matters", sub: "Debbie Millman", facts: ["Weekly", "~60 min"], rating: "4.6", reviews: "7k", note: "Long interviews", pic: eventPic("#C8674E") },
      { kind: "podcast", title: "Clever", sub: "Amy Devers", facts: ["Biweekly", "~50 min"], rating: "4.7", reviews: "1k", note: "Designers' stories", pic: eventPic("#3B6E8F") },
    ] },
};
const recFact = f => `<span style="font-size:11px;font-weight:500;padding:1px 7px;border-radius:999px;background:var(--m-t1);color:var(--m-ink)">${f}</span>`;
const recBtn = (label, toast) => `<button style="all:unset;cursor:pointer;flex:1;text-align:center;font-size:12.5px;font-weight:600;padding:7px 0;border-radius:9px;border:1.5px solid var(--m-ledge);background:var(--m-surface);color:var(--m-ink)" data-toast="${toast}">${label}</button>`;
function recCard(c, o = {}) {
  const known = !o.base && ["place", "media", "event"].includes(c.kind);
  const tall = known && c.kind === "media";
  const ph = o.short ? 72 : 108;
  // A card that came without a picture drops the picture area; an event keeps it for its date.
  const pic = !o.loading && !c.pic && !(known && c.kind === "event") ? ""
    : o.loading ? `<div style="height:${tall ? 150 : ph}px;background:var(--m-code)"></div>`
    : tall ? `<div style="display:flex;gap:0;height:${o.short ? 120 : 150}px;background:var(--m-t1)"><div style="width:${o.short ? 80 : 100}px;flex:none">${c.pic}</div>
        <div style="flex:1;display:grid;align-content:end;padding:10px;gap:4px"><span class="muted" style="font-size:10.5px">Watch on</span><b style="font-size:13px;font-weight:600;line-height:16px">${c.watch}</b></div></div>`
    : `<div style="height:${ph}px;position:relative">${c.pic}${known && c.kind === "event" ? `<span style="position:absolute;left:10px;top:10px;display:grid;justify-items:center;width:42px;padding:4px 0;border-radius: 8px;background:var(--m-surface);border:1.5px solid var(--m-ledge);color:var(--m-ink);line-height:1.1"><span style="font-size:9.5px;font-weight:600;color:var(--m-accent)">${c.when[0]}</span><b style="font-size:17px;font-weight:700">${c.when[1]}</b></span>` : ""}</div>`;
  const status = known && c.kind === "place" ? `<span style="font-size:12px;font-weight:500;color:${c.open[1] ? "var(--m-ok)" : "var(--m-muted)"}">${c.open[0]}</span>`
    : known && c.kind === "event" ? `<span style="font-size:12px;font-weight:500">${c.when[0][0] + c.when[0].slice(1).toLowerCase()} ${c.when[1]} Oct, ${c.when[2]}</span>` : "";
  const meta = [c.rating ? `${c.rating} &#9733;${c.reviews ? ` (${c.reviews})` : ""}` : "", c.note].filter(Boolean).join(" &middot; ");
  const acts = !known || o.short ? "" : c.kind === "place" ? recBtn("Directions", "Opening Google Maps navigation") + recBtn("Book", "Opens the booking page")
    : c.kind === "event" ? recBtn("Calendar", "Added to your calendar") + recBtn("Tickets", "Opens the ticket page")
    : recBtn("Open " + c.watch.replace("Rent on ", ""), "Opening " + c.watch.replace("Rent on ", ""));
  return `<div style="flex:none;width:${o.wide ? "100%" : "216px"};scroll-snap-align:start;display:grid;border:1.5px solid var(--m-ledge);border-radius:16px;overflow:hidden;background:var(--m-surface);box-shadow:0 3px 0 var(--m-ledge)">
    <button style="all:unset;cursor:pointer;display:grid" data-toast="${known && c.kind === "place" ? "Opens the place in Maps" : "Opens the page in the built-in browser"}" aria-label="${esc(c.title)}">
      ${pic}
      <span style="display:grid;gap:5px;padding:10px 12px ${acts ? 8 : 12}px">
        <span style="font-size:14px;line-height:18px;font-weight:600;white-space:nowrap;overflow:hidden;text-overflow:ellipsis">${c.title}</span>
        <span class="muted" style="font-size:12px;line-height:16px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis">${c.sub}</span>
        <span style="display:flex;gap:4px;flex-wrap:wrap">${c.facts.map(recFact).join("")}</span>
        ${status}
        ${meta ? `<span class="muted" style="font-size:12px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis">${meta}</span>` : ""}
      </span></button>
    ${acts ? `<div style="display:flex;gap:6px;padding:0 12px 12px">${acts}</div>` : ""}
  </div>`;
}

// Desktop: the same cards in the bar, shorter and without the buttons; a click opens the page, or Maps for a place.
Object.assign(S.d, { rec: "places" });
D.dRecs = () => {
  const set = RECS[S.d.rec];
  return deskScene(dBar(D_ASK + `<div class="dbody">${dYou(set.ask)}
    ${dFold([set.chip[1]])}
    <div class="dcoderen agent md">${set.say}</div>
    <div style="flex:none;display:flex;gap:10px;overflow-x:auto;scrollbar-width:none;padding-bottom:4px;align-items:start;--m-surface:#1F2023;--m-ink:#E8EEEA;--m-ledge:#34363C;--m-muted:#8A8F98;--m-code:#25262A;--m-t1:#25262A;--m-ok:#4CB782;color:var(--m-ink)">${set.cards.map(c => recCard(c, { short: true })).join("").replace(/Opens the place in Maps/g, "Opens the place in Apple Maps")}</div>
  </div>` + dFollow + dHints()));
};
DESK_SCREENS[0].items.push(["dRecs", "Recommendation cards", "RecRow"]);
VARIANTS.dRecs = [["Places", "places"], ["Films", "media"], ["Events", "events"]].map(([l, v]) => [l, () => { S.d.rec = v; render(); }, () => S.d.rec === v]);
