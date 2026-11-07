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
const camIc = '<svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M4 8h3l2-3h6l2 3h3v11H4z"/><circle cx="12" cy="13" r="3.5"/></svg>';
const flipIc = '<svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 12a8 8 0 0114-5l2 2M20 12a8 8 0 01-14 5l-2-2"/><path d="M20 4v5h-5M4 20v-5h5"/></svg>';
const boltIc = '<svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M13 3L5 14h6l-1 7 8-11h-6z"/></svg>';
// The composer with the image button, and the photos waiting to be sent above it.
function photoComposer(n, ph = "Message") {
  const thumbs = n ? `<div style="flex:none;display:flex;gap:8px;padding:8px 14px 0">${RECENT.slice(0, n).map(([k, w], i) => `<div style="position:relative;width:64px;height:64px;border-radius:var(--radius);overflow:hidden;border:1.5px solid var(--m-ledge)">${plantPic(k, w)}
      <button class="ib" style="position:absolute;top:3px;right:3px;width:20px;height:20px;border-radius:50%;background:var(--m-ink);color:var(--m-page)" data-set="cam:${n > 1 ? "attached1" : "sheet"}" aria-label="Remove photo ${i + 1}">${I.x.replace(/width="16" height="16"/, 'width="10" height="10"')}</button></div>`).join("")}</div>` : "";
  const typed = ph !== "Message";
  return thumbs + `<div class="composer"><div class="input" style="padding-right:6px">${typed ? `<span style="color:var(--m-ink);flex:1">${ph}</span>` : `<span style="flex:1">${n > 1 ? "Ask about the photos..." : n ? "Ask about the photo..." : ph}</span>`}
    ${typed ? "" : `<button class="ib" style="width:36px;height:36px;color:var(--m-muted)" data-set="cam:sheet" aria-label="Add a photo">${IC.image}</button>`}</div>
    ${n || typed ? `<button class="round" style="background:var(--m-brand);color:#fff" data-set="cam:sent" aria-label="Send">${IC.up}</button>` : `<button class="round coral" aria-label="Talk">${I.mic}</button>`}</div>`;
}
// A file waiting in the composer: icon, name, size, and a remove button.
const pFileChip = (name, size) => `<div style="position:relative;display:flex;align-items:center;gap:8px;max-width:230px;padding:7px 30px 7px 7px;border-radius:var(--radius);border:1.5px solid var(--m-ledge);background:var(--m-surface)">
  <span style="flex:none;width:32px;height:32px;border-radius:8px;display:grid;place-items:center;background:var(--m-t0)">${DOC}</span>
  <span style="min-width:0;display:grid;line-height:16px"><span style="font-size:13px;font-weight:600;white-space:nowrap;overflow:hidden;text-overflow:ellipsis">${name}</span><span class="muted" style="font-size:11px">${size}</span></span>
  <button class="ib" style="position:absolute;top:3px;right:3px;width:20px;height:20px;border-radius:50%;background:var(--m-ink);color:var(--m-page)" data-set="cam:sheet" aria-label="Remove ${name}">${I.x.replace(/width="16" height="16"/, 'width="10" height="10"')}</button></div>`;
// A file in a sent prompt opens like a file Beep sent.
const pFileRow = (name, size) => `<div style="display:flex;align-items:center;gap:10px;padding:8px 10px 8px 8px;border-radius:var(--radius);background:var(--m-surface);cursor:pointer" data-toast="Opens in a PDF viewer">
  <span style="flex:none;width:32px;height:32px;border-radius:8px;display:grid;place-items:center;background:var(--m-t0)">${DOC}</span>
  <span style="min-width:0;display:grid;line-height:16px"><span style="font-size:13px;font-weight:600;white-space:nowrap;overflow:hidden;text-overflow:ellipsis">${name}</span><span class="muted" style="font-size:11px">${size}</span></span></div>`;
R.camera = () => {
  const st = S.cam;
  // Files: picked from the system file picker, any type, up to 5 MB in all.
  if (st === "files" || st === "fileSent") {
    const head = chatBar() + `<div class="body"><div class="user">Can you check my new lease before I sign?</div>
      <div class="reply">Sure, send it over and I'll go through the deposit, notice period and anything unusual.</div>`;
    if (st === "fileSent") return head + `<div class="user" style="display:grid;gap:8px;padding:6px 6px 10px;width:250px">
        ${pFileRow("Lease 2026.pdf", "PDF, 240 KB")}${pFileRow("rent.csv", "CSV, 3 KB")}<span style="padding:0 8px">Here it is</span></div>
      <div class="reply">Two things stand out. The deposit is three months' rent, more than the usual two, and the notice period is 90 days for you but 30 for the landlord. Your rent history in the CSV is on time every month, which is worth mentioning if you ask them to lower the deposit.</div>
      </div>` + photoComposer(0);
    return head + `</div><div style="flex:none;display:flex;flex-wrap:wrap;gap:8px;padding:8px 14px 0">${pFileChip("Lease 2026.pdf", "PDF, 240 KB")}${pFileChip("rent.csv", "CSV, 3 KB")}</div>
      <div class="composer"><div class="input" style="padding-right:6px"><span style="color:var(--m-ink);flex:1">Here it is</span></div>
      <button class="round" style="background:var(--m-brand);color:#fff" data-set="cam:fileSent" aria-label="Send">${IC.up}</button></div>`;
  }
  const chatHead = chatBar() + `<div class="body"><div class="user">My calathea keeps getting crispy brown edges</div>
    <div class="reply">Usually that's dry air or hard tap water. Can you show me a leaf up close, and the soil?</div>`;
  if (st === "sent") return chatHead + `<div class="user" style="padding:4px;display:grid;grid-template-columns:1fr 1fr;gap:4px;width:220px">
      ${[0, 1].map(i => `<div style="height:130px;border-radius:var(--radius);overflow:hidden">${plantPic(RECENT[i][0], RECENT[i][1])}</div>`).join("")}
      <span style="grid-column:1/-1;padding:4px 10px 6px">Here</span></div>
    <div class="reply">The brown starts at the tips and the soil has pulled away from the pot, so it dried out completely. Water until it runs out the bottom, then again when the top 2 cm feel dry. Filtered or rain water helps with the edges.</div>
    </div>` + photoComposer(0);
  if (st === "attached" || st === "attached1") { const n = st === "attached" ? 2 : 1;
    return chatHead + `</div>` + photoComposer(n); }
  // Full screen camera inside the chat. Each shot adds to the count; Done puts them all in the composer.
  if (st === "camera" || st === "review" || st === "shot") {
    const shots = st === "shot" ? 1 : 0;
    if (st === "review") return `<div style="position:absolute;inset:0;background:#000;z-index:9;display:flex;flex-direction:column;color:#fff">
      <div class="bar" style="padding-left:6px"><button class="ib" style="color:#fff" data-set="cam:camera" aria-label="Retake">${I.back}</button><div style="flex:1;font-size:14px;color:#ccc">Photo 2</div></div>
      <div style="flex:1;min-height:0;margin:0 0 8px">${plantPic(1, "#E4E8E2")}</div>
      <div style="display:flex;gap:10px;padding:8px 16px 22px">
        <button class="pill" style="flex:1;background:transparent;color:#fff;border-color:#fff" data-set="cam:camera">Retake</button>
        <button class="pill chunky coral" style="flex:1;font-weight:600;color:var(--m-ink)" data-set="cam:attached">Use 2 photos</button></div></div>`;
    return `<div style="position:absolute;inset:0;background:#000;z-index:9;display:flex;flex-direction:column;color:#fff">
      <div style="flex:1;min-height:0;position:relative">${plantPic(2, "#DCD5C8")}
        <div style="position:absolute;inset:0;background:linear-gradient(#0008,transparent 18%,transparent 70%,#000a)"></div>
        <div class="bar" style="position:absolute;top:0;left:0;right:0;padding:0 8px">
          <button class="ib" style="color:#fff" data-set="cam:sheet" aria-label="Close camera">${I.x}</button><div style="flex:1"></div>
          <button class="ib" style="color:#fff" data-toast="Flash on" aria-label="Flash">${boltIc}</button></div>
        <div style="position:absolute;left:50%;top:44%;width:70px;height:70px;transform:translate(-50%,-50%);border:2px solid #fffc;border-radius:var(--radius)"></div>
        <div style="position:absolute;left:0;right:0;bottom:12px;display:flex;justify-content:center;gap:16px;font-size:13px;font-weight:600">
          <span style="color:#fff9">0.5×</span><span style="color:var(--m-accent)">1×</span><span style="color:#fff9">2×</span></div></div>
      <div style="flex:none;display:flex;align-items:center;justify-content:space-between;padding:18px 28px 26px">
        <button style="all:unset;cursor:pointer;width:48px;height:48px;border-radius:var(--radius);overflow:hidden;border:2px solid #fff;position:relative" data-set="cam:${shots ? "attached1" : "sheet"}" aria-label="${shots ? "1 photo taken" : "Recent photos"}">
          ${plantPic(RECENT[0][0], RECENT[0][1])}${shots ? `<span style="position:absolute;right:2px;bottom:2px;min-width:18px;height:18px;border-radius:9px;background:var(--m-accent);color:var(--m-ink);font-size:11px;font-weight:700;display:grid;place-items:center">1</span>` : ""}</button>
        <button style="all:unset;cursor:pointer;width:72px;height:72px;border-radius:50%;border:4px solid #fff;display:grid;place-items:center" data-set="cam:${shots ? "review" : "shot"}" aria-label="Take photo"><span style="width:58px;height:58px;border-radius:50%;background:#fff"></span></button>
        <button class="ib" style="width:48px;height:48px;border-radius:50%;background:#ffffff26;color:#fff" data-toast="Front camera" aria-label="Switch camera">${flipIc}</button></div>
      ${shots ? `<button class="text-btn" style="position:absolute;right:16px;bottom:118px;color:#fff;background:#0008;border-radius:999px;padding:0 14px" data-set="cam:attached1">Done</button>` : ""}</div>`;
  }
  // The sheet: camera tile live, then recent photos to pick from, and the full gallery.
  const perm = st === "perm";
  const picked = st === "picked";
  const tile = perm ? `<button style="all:unset;cursor:pointer;aspect-ratio:1;border-radius:var(--radius);background:var(--m-t2);border:1.5px solid var(--m-ledge);display:grid;place-items:center;text-align:center;font-size:11px;line-height:14px;padding:6px" data-set="cam:camera" data-toast="Camera allowed">${camIc}<span>Allow camera</span></button>`
    : `<button style="all:unset;cursor:pointer;aspect-ratio:1;border-radius:var(--radius);overflow:hidden;position:relative" data-set="cam:camera" aria-label="Take a photo">${plantPic(2, "#DCD5C8")}
        <span style="position:absolute;inset:0;background:#0006;display:grid;place-items:center;color:#fff">${camIc}</span></button>`;
  const recent = RECENT.slice(0, 8).map(([k, w], i) => `<button style="all:unset;cursor:pointer;aspect-ratio:1;border-radius:var(--radius);overflow:hidden;position:relative${picked && i < 2 ? ";outline:3px solid var(--m-brand);outline-offset:-3px" : ""}" data-set="cam:picked" aria-label="Photo ${i + 1}">${plantPic(k, w)}
      ${picked && i < 2 ? `<span style="position:absolute;top:5px;right:5px;width:20px;height:20px;border-radius:50%;background:var(--m-brand);color:#fff;font-size:11px;font-weight:700;display:grid;place-items:center">${i + 1}</span>` : ""}</button>`).join("");
  return chatHead + `</div>` + photoComposer(0) + `<div class="scrim" data-set="cam:attached1" style="z-index:8;background:rgba(16,32,26,.3)"></div>
    <div class="sheet" role="dialog" aria-label="Add a photo" style="gap:12px">
      <div class="grab"></div>
      <div style="display:flex;gap:10px">
        <button class="pill chunky" style="flex:1;display:flex;align-items:center;justify-content:center;gap:8px;font-weight:600" data-toast="System photo picker">${IC.image}All photos</button>
        ${picked ? "" : `<button class="pill chunky" style="flex:1;display:flex;align-items:center;justify-content:center;gap:8px;font-weight:600" data-set="cam:files" aria-label="Attach files">${DOC}Files</button>`}
        ${picked ? `<button class="pill chunky coral" style="flex:1;font-weight:600" data-set="cam:attached">Add 2</button>` : ""}</div>
      <div style="display:grid;grid-template-columns:repeat(3,1fr);gap:6px">${tile}${recent}</div>
      ${perm ? `<div class="muted" style="font-size:12.5px;line-height:18px">Beep only uses the camera while it's open here. Photos you take are sent with your message and not saved to the phone.</div>` : ""}
    </div>`;
};
SCREENS.find(g => g.group === "Chat").items.splice(4, 0, ["camera", "Photos and files", "AttachSheet, ChatCamera"]);
VARIANTS.camera = vs("cam", [["Sheet", "sheet"], ["Camera off", "perm"], ["Picked recent", "picked"], ["Files", "files"], ["File sent", "fileSent"], ["Camera", "camera"], ["One shot", "shot"], ["Review", "review"], ["In composer", "attached"], ["Sent", "sent"]]);
