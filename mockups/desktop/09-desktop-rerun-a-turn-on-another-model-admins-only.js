// ---- Desktop: rerun a turn on another model (admins only) ----
// The menu sits beside Copy under the reply. A rerun drops the turn and every later one, then sends the prompt again on the picked model.
D.dRerun = () => {
  const st = S.d.rerun || "open", ask = "What's a good way to ask for the deposit back?";
  const pickId = S.d.rerunModel || "GPT-6 Luna", think = S.d.rerunThink || "Default";
  const first = "Keep it short and specific. Say when you moved out, that the flat was left in good condition, and ask for the full deposit by a date two weeks out. Attach the move-out photos.";
  const again = "Write a short email: the date you handed back the keys, that the flat was left clean and undamaged, and the amount you expect back by a set date. Attach the inventory and your move-out photos so there is nothing to dispute.";
  const menu = st === "open" ? `<div class="dmenu" role="menu" aria-label="Rerun on">${D_MODELS.map(([n, d], i) => `<button class="mi${i === 1 ? " hl" : ""}" role="menuitem" data-dact="rerun:${n}"${i === 0 ? ' aria-current="true"' : ""}><b>${n}</b>${i === 0 ? `<span class="ok">${DI.check}</span>` : ""}<small>${i === 0 ? "Used for this reply" : d}</small></button>`).join("")}
      <div class="think">Thinking<span class="dchips" style="display:flex;gap:4px">${["Default", "Off", "Low", "High"].map(l => `<button class="dchip" aria-pressed="${l === think}" data-dact="rthink:${l}">${l}</button>`).join("")}</span></div></div>` : "";
  const acts = (open) => `<div class="dreplyacts"><button class="dcopy">${D_COPY}Copy</button><span class="drerun"><button class="dcopy" data-dact="rerun:${open ? "close" : "open"}" aria-haspopup="menu" aria-expanded="${open}">${I.play.replace('width="16" height="16"', 'width="12" height="12"')}Rerun<span style="display:grid;transform:rotate(90deg)">${DI.chev}</span></button>${open ? menu : ""}</span></div>`;
  const tag = `<div class="drerunof">Rerun on ${pickId}${think === "Default" ? "" : " · " + think.toLowerCase()}</div>`;
  const body = st === "open" || st === "closed"
    ? dLog(`${dYou(ask)}${dHead("Worked for 6s")}<div class="dcoderen agent md">${first}</div>${acts(st === "open")}`)
    : dLog(`${dYou(ask)}${tag}${st === "running" ? dHead("Working for 1s") + dRows(dThink({ live: true })) : `${dHead("Worked for 7s")}<div class="dcoderen agent md">${again}</div>${acts(false)}`}`);
  return deskScene(dBar(D_ASK + `<div class="dbody"${st === "open" ? ' style="min-height:350px"' : ""}>${body}</div>` + dFollow + dHints()));
};
VARIANTS.dRerun = [
  ["Menu open", () => { S.d.rerun = "open"; render(); }, () => (S.d.rerun || "open") === "open"],
  ["Rerunning", () => { S.d.rerun = "running"; render(); }, () => S.d.rerun === "running"],
  ["Rerun done", () => { S.d.rerun = "done"; render(); }, () => S.d.rerun === "done"],
];
