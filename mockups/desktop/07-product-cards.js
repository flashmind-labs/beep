// ---- Product cards ----
// Beep calls show_products (title, url, store, price and currency, image, rating, note). The server checks the
// links and images, and the cards show under the reply in a row that scrolls sideways. The app writes the price
// in the phone's own format. Tapping a card opens the shop in the built-in browser, the corner button in the phone's own browser.
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
  { title: "Bosch Unlimited 7 ProAnimal", store: "mediamarkt.es", price: "€299", was: "€349", rating: "4.5", reviews: "912", body: "#4C56B0", note: "Swappable battery" },
];
const extIc = '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M14 4h6v6M20 4l-9 9M18 14v5a1 1 0 01-1 1H5a1 1 0 01-1-1V7a1 1 0 011-1h5"/></svg>';
function productCard(p, o = {}) {
  const w = o.wide ? "100%" : "208px";
  const img = o.loading ? `<div style="height:116px;background:var(--m-code)"></div>` : `<div style="height:116px">${vacPic(p.body)}</div>`;
  const price = o.noPrice ? `<span class="muted" style="font-size:12.5px">See price on the site</span>`
    : `<span style="display:flex;align-items:baseline;gap:6px"><b style="font-size:17px;font-weight:600">${p.price}</b>${p.was ? `<s class="muted" style="font-size:12px">${p.was}</s>` : ""}</span>`;
  return `<div style="flex:none;width:${w};position:relative;scroll-snap-align:start">
    <button style="all:unset;cursor:pointer;display:grid;width:100%;border:1.5px solid var(--m-ledge);border-radius:16px;overflow:hidden;background:var(--m-surface);box-shadow:0 3px 0 var(--m-ledge)" data-go="browser" aria-label="${esc(p.title)}, ${p.price}, opens ${p.store}">
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

// Desktop: product cards in the bar, and pictures sent from the phone loaded from the server.
Object.assign(S.d, { prod: "row", phonePic: "loaded" });
D.dProducts = () => deskScene(dBar(D_ASK + `<div class="dbody">${dYou("Find me a quiet cordless vacuum under 300 euros")}
    ${dFold(["Searching the web"])}
    <div class="dcoderen agent md">Three that fit under €300. All are cordless with a HEPA filter; the Dreame is the quietest. Want me to keep an eye on the Bosch and tell you if it drops under €280?</div>
    <div style="flex:none;display:flex;gap:10px;overflow-x:auto;scrollbar-width:none;padding-bottom:4px;--m-surface:#1F2023;--m-ink:#E8EEEA;--m-ledge:#34363C;--m-muted:#8A8F98;--m-code:#25262A;color:var(--m-ink)">${PRODUCTS.map(p => productCard(p, { loading: S.d.prod === "loading" }).replace(/data-go="browser"/g, `data-toast="Opens ${p.store} in your browser"`).replace(/data-toast="Opens in Chrome"/, 'data-toast="Copied link"').replace(extIc, DI.copy || extIc)).join("")}</div>
  </div>` + dFollow + dHints()));
D.dPhonePics = () => {
  const st = S.d.phonePic;
  const tiles = [0, 1].map(i => st === "loading" ? `<span style="width:96px;height:120px;border-radius: 8px;background:var(--d-raised, #0001);display:block"></span>`
    : st === "gone" ? `<span style="width:96px;height:120px;border-radius: 8px;border:1px dashed currentColor;opacity:.5;display:grid;place-items:center;font-size:11px;text-align:center;padding:6px">Picture no longer available</span>`
    : `<span style="width:96px;height:120px;border-radius: 8px;overflow:hidden;display:block">${plantPic(RECENT[i][0], RECENT[i][1])}</span>`).join("");
  return deskScene(dBar(`<div class="dinput" style="align-items:flex-start"><span style="flex:1;min-width:0;display:grid;gap:8px"><span class="txt">Here</span><span style="display:flex;gap:8px">${tiles}</span>
      <span class="dmuted" style="font-size:11px">Sent from Pixel</span></span></div><div class="dbody">
    <div>The brown starts at the tips and the soil has pulled away from the pot, so it dried out completely. Water until it runs out the bottom, then again when the top 2 cm feel dry.</div>
  </div>` + dFollow + dHints()));
};
DESK_SCREENS[0].items.push(["dProducts", "Product cards", "ProductRow"], ["dPhonePics", "Pictures from the phone", "PromptBar, synced images"]);
VARIANTS.dProducts = [["Row", () => { S.d.prod = "row"; render(); }, () => S.d.prod === "row"], ["Images loading", () => { S.d.prod = "loading"; render(); }, () => S.d.prod === "loading"]];
VARIANTS.dPhonePics = [["Loaded", () => { S.d.phonePic = "loaded"; render(); }, () => S.d.phonePic === "loaded"], ["Loading", () => { S.d.phonePic = "loading"; render(); }, () => S.d.phonePic === "loading"], ["Expired", () => { S.d.phonePic = "gone"; render(); }, () => S.d.phonePic === "gone"]];

// Desktop: image files Beep sends (send_image or send_file) show as pictures, not file cards.
// Hover shows Save; a click opens the viewer inside the bar, with arrows, Save and Copy.
const RPICS = [
  ["1_bit_vs_qubit.png", "PNG, 65 KB", "A bit is 0 or 1, a qubit is a direction"],
  ["2_interference.png", "PNG, 123 KB", "Waves that line up add, opposite ones cancel"],
  ["3_shor_period.png", "PNG, 37 KB", "The hidden repeat Shor finds"],
];
function rchart(i, w = 320, h = 200) {
  const ax = `<line x1="30" y1="${h - 30}" x2="${w - 20}" y2="${h - 30}" stroke="#8A8F98"/><line x1="30" y1="20" x2="30" y2="${h - 30}" stroke="#8A8F98"/>`;
  const body = [
    `<rect x="60" y="70" width="70" height="70" rx="8" fill="none" stroke="#0F1011" stroke-width="2"/><text x="95" y="113" text-anchor="middle" font-size="22" fill="#0F1011">0</text>
     <circle cx="230" cy="105" r="45" fill="none" stroke="#5E6AD2" stroke-width="2"/><line x1="230" y1="105" x2="262" y2="73" stroke="#F2994A" stroke-width="3"/>`,
    `${ax}<path d="M30 100 C60 30 90 30 120 100 S180 170 210 100 S270 30 300 100" fill="none" stroke="#5E6AD2" stroke-width="3"/>
     <path d="M30 100 C60 60 90 60 120 100 S180 140 210 100 S270 60 300 100" fill="none" stroke="#F2994A" stroke-width="2" stroke-dasharray="6 5"/>`,
    `${ax}${[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11].map(k => `<rect x="${40 + k * 22}" y="${[60, 110, 140, 90][k % 4]}" width="14" height="${170 - [60, 110, 140, 90][k % 4]}" fill="${k % 4 ? "#CFE0D6" : "#5E6AD2"}"/>`).join("")}`,
  ][i];
  return `<svg viewBox="0 0 ${w} ${h}" preserveAspectRatio="xMidYMid meet" style="display:block;width:100%;height:100%;background:#fff">${body}</svg>`;
}
const D_SAVE = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 4v11M7 10l5 5 5-5M5 20h14"/></svg>';
Object.assign(S.d, { rpic: "inline", rpicAt: 1 });
D.dReplyPics = () => {
  const st = S.d.rpic, at = S.d.rpicAt;
  const tile = ([name, about], i) => {
    if (st === "loading") return `<span class="drpic wait"></span>`;
    if (st === "gone" && i === 2) return `<span class="drpic gone">Picture no longer available</span>`;
    return `<span class="drpic${st === "inline" && i === 1 ? " hov" : ""}"><button class="pic" data-dact="rpic:viewer:${i}" aria-label="Open ${esc(name)}">${rchart(i)}</button>
      <button class="save" data-toast="Saved ${esc(name)} to Downloads" aria-label="Save ${esc(name)}">${D_SAVE}</button></span>`;
  };
  const [name, about, cap] = RPICS[at];
  const viewer = st === "viewer" ? `<div class="dviewer" role="dialog" aria-label="${esc(name)}">
      <div class="vt"><span class="tt"><b>${esc(name)}</b><small>${at + 1} of ${RPICS.length} · ${about}</small></span>
        <button class="dbtn" data-toast="Copied the picture">${D_COPY}Copy<kbd>⌘C</kbd></button>
        <button class="dbtn pri" data-dact="rpic:saved">${D_SAVE}Save<kbd>⌘S</kbd></button>
        <button class="dbtn" data-dact="rpic:inline" aria-label="Close">${DI.x}</button></div>
      <div class="vi"><button class="nav" data-dact="rpic:prev" aria-label="Previous" ${at ? "" : "disabled"}><span style="display:grid;transform:rotate(180deg)">${DI.chevR}</span></button>
        <div class="img">${rchart(at)}</div>
        <button class="nav" data-dact="rpic:next" aria-label="Next" ${at < RPICS.length - 1 ? "" : "disabled"}>${DI.chevR}</button></div>
      <div class="cap">${esc(cap)}</div>${dHints([["← →", "move"], ["⌘S", "save"], ["⌘C", "copy"], ["esc", "close"]])}
    </div>` : "";
  const saved = st === "saved" ? `<div class="dtoast" style="display:flex;gap:12px;align-items:center;bottom:96px;padding:5px 6px 5px 14px">Saved ${esc(name)} to Downloads<button class="dbtn" style="min-height:24px" data-toast="Opens Finder">Show in Finder</button></div>` : "";
  return deskScene(dBar(D_ASK + `<div class="dbody">${dYou("Explain how a quantum computer breaks RSA, with drawings")}
      ${dFold(["Ran code", "Sending pictures"])}
      <div class="dcoderen agent md">Shor's algorithm turns breaking the key into finding a hidden repeat. The second drawing is the key idea: interference.</div>
      <div class="drpics">${RPICS.map(tile).join("")}</div>
      <div>If it helps, the one sentence version: <b>a qubit is a direction, the algorithm bends those directions so wrong answers cancel, and Shor uses that to find the repeat that gives up your key.</b></div>
    </div>` + viewer + dFollow + saved + dHints()));
};
DESK_SCREENS[0].items.push(["dReplyPics", "Pictures in a reply", "ReplyImages, ImageViewer"]);
VARIANTS.dReplyPics = [
  ["Inline", () => { S.d.rpic = "inline"; render(); }, () => S.d.rpic === "inline"],
  ["Loading", () => { S.d.rpic = "loading"; render(); }, () => S.d.rpic === "loading"],
  ["One gone", () => { S.d.rpic = "gone"; render(); }, () => S.d.rpic === "gone"],
  ["Viewer", () => { S.d.rpic = "viewer"; render(); }, () => S.d.rpic === "viewer"],
  ["Saved", () => { S.d.rpic = "saved"; render(); }, () => S.d.rpic === "saved"],
];

// Desktop: copying a reply or a code block, and the model picker admins see.
Object.assign(S.d, { model: "pick", copied: "", agents: false });
const D_COPY = '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="8" y="8" width="12" height="12" rx="2"/><path d="M16 8V5a1 1 0 00-1-1H5a1 1 0 00-1 1v10a1 1 0 001 1h3"/></svg>';
const D_MODELS = [["Claude Haiku 5.5", "$1 in, $5 out per M tokens", true], ["GPT-6 Luna", "$1.25 in, $10 out per M tokens", false], ["GLM 5.3 Flash", "$0.2 in, $0.8 out per M tokens, text only", false]];
D.dCopy = () => {
  const c = S.d.copied;
  const btn = (k, label) => `<button class="dcopy" data-dact="copy:${k}">${c === k ? DI.check : D_COPY}${label ? (c === k ? "Copied" : label) : ""}</button>`;
  return deskScene(dBar(D_ASK + `<div class="dbody">${dYou("How do I list the five biggest files in a folder?")}${dFold(["Searching the web"], 3)}
    <div class="dcoderen agent md">Sort by size and keep the top five:</div>
    <pre class="dpre"><code>du -ah . | sort -rh | head -n 5</code>${btn("code")}</pre>
    <div class="dcoderen agent md"><code>du -a</code> lists files as well as folders, and <code>-h</code> keeps the sizes readable.</div>
    <div class="dreplyacts">${btn("reply", "Copy")}</div>
  </div>` + dHints()));
};
// Only when GetModels lists choices, so only admins see the chip and the picker. The choice applies to every turn on this account, like the phone's setting.
D.dModel = () => {
  const st = S.d.model, auto = st === "auto";
  const chip = `<button class="dmodel" data-dact="model:pick">${auto ? "Auto" : "Claude Haiku 5.5 · Medium"}<kbd>⌘E</kbd></button>`;
  if (st === "hints") return deskScene(dBar(`<div class="dinput" style="align-items:flex-start"><span style="flex:1;min-width:0;display:grid;gap:4px"><span class="txt">Summarise the last three emails from Alex</span><span class="dmuted" style="font-size:11px">GPT-6 Luna · Low</span></span></div><div class="dbody">
      ${dFold(["Gmail: fetch emails"])}
      <div class="dcoderen agent md">Alex confirmed the plumber for Thursday 9 to 12, asked for the door code, and sent the new lease to sign by Friday.</div>
    </div>` + dFollow + dHints().replace('<span class="st">', `${chip.replace("Claude Haiku 5.5 · Medium", "Auto")}<span class="st">`)));
  const rows = D_MODELS.map(([n, d, on]) => `<button class="drt dmrow" aria-selected="${on}" role="option"><span class="tt"><b>${n}</b><span>${d}</span></span>${on ? `<span class="dok">${DI.check}</span>` : ""}</button>`).join("");
  // With Agents on, the agent Jev picks brings the model, so Auto on leaves nothing to choose here.
  const ag = S.d.agents;
  const autoSub = auto ? (ag ? "Jev picks an agent for each prompt" : "Jev picks the thinking for each prompt")
    : (ag ? "Use the model below, with the agent's prompt" : "Off: every prompt uses the model below");
  return deskScene(dBar(`<div class="dhead"><span class="tt"><b>Model</b><small>Your own turns only</small></span></div>
    <div class="dform" style="padding-top:12px">
      <div class="dtoggle"><span><b>Auto</b><small>${autoSub}</small></span><button class="dswitch" role="switch" aria-checked="${auto}" data-dact="model:${auto ? "pick" : "auto"}"></button></div>
      ${`<div class="dlist" style="padding:0">${rows}</div>
      <div class="dfield"><span>Thinking</span><div class="dchips">${["Off", "Low", "Medium", "High"].map(l => `<button class="dchip" aria-pressed="${l === "Medium"}">${l}</button>`).join("")}</div></div>`}
    </div>` + dHints([["↑↓", "move"], [DI.ret, "use"], ["esc", "back"]])));
};
DESK_SCREENS[0].items.push(["dCopy", "Copy a reply", "PromptBar, copy"], ["dModel", "Model (admin)", "ModelPicker"]);
VARIANTS.dCopy = [["Idle", () => { S.d.copied = ""; render(); }, () => !S.d.copied], ["Code copied", () => { S.d.copied = "code"; render(); }, () => S.d.copied === "code"], ["Reply copied", () => { S.d.copied = "reply"; render(); }, () => S.d.copied === "reply"]];
VARIANTS.dModel = [["Picker", () => { S.d.model = "pick"; S.d.agents = false; render(); }, () => S.d.model === "pick" && !S.d.agents], ["Auto on", () => { S.d.model = "auto"; S.d.agents = false; render(); }, () => S.d.model === "auto" && !S.d.agents],
  ["Agents, Auto on", () => { S.d.model = "auto"; S.d.agents = true; render(); }, () => S.d.model === "auto" && S.d.agents], ["Agents, Auto off", () => { S.d.model = "pick"; S.d.agents = true; render(); }, () => S.d.model === "pick" && S.d.agents], ["Chip and Auto pick", () => { S.d.model = "hints"; render(); }, () => S.d.model === "hints"]];
