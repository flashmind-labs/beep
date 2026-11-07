// ---- Product cards ----
// Beep calls show_products (title, url, store, price and currency, image, rating, note). The server checks the
// links and images, and the cards show under the reply in a row that scrolls sideways. The app writes the price
// in the phone's own format. Tapping a card opens the shop in the phone's own browser.
// Tools whose result is already on screen or is bookkeeping (show_products, notify_user, suggest_replies,
// task_progress) get no chip or step, on the phone or the desktop.
Object.assign(S, { prod: "row", brw: "chrome", notif: "shade" });
// Made-up product shots drawn as SVG: a stick vacuum in three colours.
const vacPic = (body = "#C8674E", bg = "#F1EDE6") => `<svg viewBox="0 0 160 120" preserveAspectRatio="xMidYMid meet" style="display:block;width:100%;height:100%">
  <rect width="160" height="120" fill="${bg}"/><rect x="76" y="10" width="6" height="78" rx="3" fill="#8E8A84"/>
  <rect x="64" y="22" width="30" height="34" rx="10" fill="${body}"/><circle cx="79" cy="39" r="7" fill="#fff" opacity=".55"/>
  <rect x="88" y="14" width="10" height="18" rx="4" fill="#3A3F3C"/><path d="M58 92h44l4 10H54z" fill="#3A3F3C"/><rect x="74" y="86" width="10" height="8" fill="#5A5F5C"/></svg>`;
const PRODUCTS = [
  { title: "Dreame Z30 cordless stick vacuum", store: "amazon.es", price: "€249", was: "€329", rating: "4.6", reviews: "1,204", body: "#C8674E", note: "Quietest of the three" },
  { title: "Rowenta X-Force Flex 9.60 Allergy", store: "elcorteingles.es", price: "€279", was: "", rating: "4.4", reviews: "386", body: "#3B6E8F", note: "Bends under furniture" },
  { title: "Bosch Unlimited 7 ProAnimal", store: "mediamarkt.es", price: "€299", was: "€349", rating: "4.5", reviews: "912", body: "#2F7D5B", note: "Swappable battery" },
];
const extIc = '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M14 4h6v6M20 4l-9 9M18 14v5a1 1 0 01-1 1H5a1 1 0 01-1-1V7a1 1 0 011-1h5"/></svg>';
function productCard(p, o = {}) {
  const w = o.wide ? "100%" : "208px";
  const img = o.loading ? `<div style="height:116px;background:var(--m-code)"></div>` : `<div style="height:116px">${vacPic(p.body)}</div>`;
  const price = o.noPrice ? `<span class="muted" style="font-size:12.5px">See price on the site</span>`
    : `<span style="display:flex;align-items:baseline;gap:6px"><b style="font-size:17px;font-weight:600">${p.price}</b>${p.was ? `<s class="muted" style="font-size:12px">${p.was}</s>` : ""}</span>`;
  return `<div style="flex:none;width:${w};position:relative;scroll-snap-align:start">
    <button style="all:unset;cursor:pointer;display:grid;width:100%;border:1.5px solid var(--m-ledge);border-radius:var(--radius);overflow:hidden;background:var(--m-surface);box-shadow:0 3px 0 var(--m-ledge)" data-toast="Opens in Safari" aria-label="${esc(p.title)}, ${p.price}, opens ${p.store}">
      ${img}
      <span style="display:grid;gap:5px;padding:10px 12px 12px">
        <span class="muted" style="font-size:11px;letter-spacing:.02em">${p.store}</span>
        <span style="font-size:13.5px;line-height:18px;font-weight:500;display:-webkit-box;-webkit-line-clamp:2;-webkit-box-orient:vertical;overflow:hidden;min-height:36px">${p.title}</span>
        ${price}
        <span class="muted" style="font-size:12px">${p.rating} &#9733; (${p.reviews})${p.note ? ` &middot; ${p.note}` : ""}</span>
      </span></button>
    <button class="ib" style="position:absolute;top:8px;right:8px;width:30px;height:30px;border-radius:50%;background:var(--m-surface);color:var(--m-ink);border:1.5px solid var(--m-ledge)" data-toast="Opens in Chrome" aria-label="Open in your browser">${extIc}</button>
  </div>`;
}
R.products = () => {
  const st = S.prod;
  const cards = st === "single" ? productCard(PRODUCTS[0], { wide: true })
    : `<div style="display:flex;gap:10px;overflow-x:auto;scroll-snap-type:x mandatory;scroll-padding:0 16px;margin:0 -16px;padding:0 16px 6px;scrollbar-width:none">${PRODUCTS.map(p => productCard(p, { loading: st === "loading", noPrice: st === "noprice" && p.store === "elcorteingles.es" })).join("")}</div>`;
  const reply = st === "single" ? "The Dreame Z30 fits, and it's the quietest at 68 dB. Want me to tell you if it drops further?"
    : "Three that fit under €300. All are cordless with a HEPA filter; the Dreame is the quietest. Want me to keep an eye on the Bosch and tell you if it drops under €280?";
  return chatBar() + `<div class="body">
    <div class="user">Find me a quiet cordless vacuum under 300 euros</div>
    <div class="chip" style="background:var(--m-t1);align-self:flex-start"><span class="ini">W</span>Searched the web<span class="st" style="color:var(--m-ok)">${I.check}</span></div>
    <div class="reply">${reply}</div>
    ${cards}
  </div>` + composer();
};
