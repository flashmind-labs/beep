// ---- Tool run ----
const CALLS = [
  ["Fastmail: search email", "F", true],
  ["Fastmail: search email", "F", false],
  ["Fastmail: read thread", "F", true],
  ["Searching the web", "W", true],
  ["Checking your location", "L", true],
  ["Fastmail: read email", "F", true],
  ["Checking the time", "T", true],
  ["Weather: forecast", "W", true],
];
const SHOWN = 3;
let timer = null;
function replay() {
  S.tools.arrived = 1; S.tools.open = false;
  if (S.screen === "chat") { render(); }
}
function startRun() {
  clearInterval(timer);
  timer = setInterval(() => {
    if (S.screen !== "chat" || S.tools.arrived > CALLS.length) return clearInterval(timer);
    S.tools.arrived++; syncTools();
  }, 900);
}
function chatHtml() {
  if (S.updChat) return topBar() + `<div class="body">
    <div style="align-self:stretch;text-align:center;font-size:11px;color:var(--m-dim)">Today</div>
    <div class="reply">${UPDATE_CHAT[2]}</div>
    <div class="muted" style="font-size:11.5px;padding:0 4px;margin-top:-6px">From ${UPDATE_CHAT[0]} · ${UPDATE_CHAT[1]}</div>
  </div>` + composer("Reply to Beep");
  const chips = CALLS.map(([label, ini], i) => `<div class="tw" data-tool="${i}"><div class="chip" style="background:${TINTS[i % 3]}"><span class="ini">${ini}</span>${esc(label)}<span class="st"></span></div></div>`).join("");
  return topBar() + `<div class="body">
    <div style="align-self:stretch;text-align:center;font-size:11px;color:var(--m-dim)">Yesterday</div>
    ${autoLabel(`<div class="user">Can you check the bins are out Thursday?</div>`, "DeepSeek V4.1 Flash · low")}
    <div class="reply">Yes, collection is Thursday morning. I'll remind you Wednesday evening.</div>
    <div style="align-self:stretch;text-align:center;font-size:11px;color:var(--m-dim)">Claude Code finished</div>
    <div class="reply">The login test is fixed and the branch is pushed.</div>
    <div style="align-self:stretch;text-align:center;font-size:11px;color:var(--m-dim)">Today</div>
    ${autoLabel(`<div class="user">Did the landlord reply about the boiler? And what's the weather like for Saturday?</div>`, "GPT-6 Luna · high")}
    <div style="display:flex;flex-direction:column">
      <button class="fold" id="fold" data-act="toggleTools"><span class="stack" id="stack"></span><span style="display:inline-flex;align-items:center"><span id="plus">+</span><span class="odo"><span id="odo">${[0,1,2,3,4,5,6,7,8].map(n => `<i>${n}</i>`).join("")}</span></span>&nbsp;<span id="unit">tools</span></span><span class="names" id="names"></span>${I.chev}</button>
      ${chips}
    </div>
    ${S.connected ? "" : `<div style="display:flex;align-items:center;gap:10px;padding:0 4px;font-size:13px;color:var(--m-muted)"><span style="display:flex;gap:4px">${[0,1,2].map(() => '<i style="width:6px;height:6px;border-radius:50%;background:var(--m-dim)"></i>').join("")}</span>Waiting for a connection…</div>`}
    <div class="answer" id="thoughts" style="margin-bottom:-8px">
      <button class="text-btn" style="color:var(--m-muted);font-size:12px;padding:0 4px;display:flex;align-items:center;gap:2px" data-act="thoughts" aria-expanded="${S.thoughts}">Thoughts${I.chev.replace('class="chev"', `class="chev" style="transform:rotate(${S.thoughts ? 180 : 0}deg)"`)}</button>
      ${S.thoughts ? `<div class="thoughts"><div class="h">Two questions</div>
        The user wants the <b>landlord's reply</b> and the <b>Saturday forecast</b>.<br>
        &bull; Alex's mail came in at 08:12, the thread says the plumber can do <i>Thursday 9 to 12</i><br>
        &bull; The forecast for <code>Saturday</code> is dry after 10:00, 19&deg;C<br>
        Lead with the boiler, it needs an answer from them.</div>` : ""}
    </div>
    <div class="answer reply" id="answer">Yes, Alex replied this morning: the plumber can come Thursday between 9 and 12, and she asked if that works for you.<br><br>Saturday looks dry in the afternoon, around 19&deg;C, with some rain before 10.</div>
    <div class="answer" id="answer2" style="display:flex;gap:6px 8px;flex-wrap:wrap;transition-delay:.15s">
      <button class="chunky rchip" data-toast="Draft ready: Thursday works">Tell her Thursday works</button>
      <button class="chunky rchip" data-toast="Reminder set for Friday 09:00">Remind me Friday</button>
    </div>
  </div>` + endedBanner() + composer();
}
// With Auto on, the model and thinking Jev picked sit under the prompt, like the memory score.
// Everyone else is always on Auto for thinking only, so they see just the level.
function autoLabel(bubble, pick) {
  if (S.admin && !S.auto) return bubble;
  if (!S.admin) pick = pick.split(" · ")[1];
  return `<div class="prompt" style="display:grid;justify-items:end;gap:4px">${bubble}<span style="font-size:11px;color:var(--m-dim);padding-right:4px">${pick}</span></div>`;
}
// A run that ended without finishing; Retry sends the last message again.
function endedBanner() {
  if (!S.ended) return "";
  const text = S.ended === "stopped" ? "Stopped" : "The server restarted before Beep finished.";
  return `<div style="margin:4px 16px;padding:4px 6px 4px 14px;border-radius:var(--radius);background:color-mix(in srgb, var(--m-error) 10%, transparent);display:flex;align-items:center;gap:8px">
    <div style="flex:1;font-size:13px;color:var(--m-error);padding:8px 0">${text}</div>
    ${S.admin ? `<button class="text-btn" style="color:var(--m-error);font-weight:600;display:flex;align-items:center;gap:2px" data-toast="Pick a model to run the prompt again on">Rerun ${I.chev}</button>` : ""}
    <button class="text-btn" style="color:var(--m-error);font-weight:600" data-act="retryRun">Retry</button></div>`;
}
function syncTools() {
  const t = S.tools, count = Math.min(t.arrived, CALLS.length), done = t.arrived > CALLS.length;
  const summed = S.summary && done;
  const folded = summed ? count : Math.max(0, count - SHOWN), hidden = t.open ? 0 : folded;
  document.querySelectorAll("[data-tool]").forEach(el => {
    const i = +el.dataset.tool, here = i < count, running = here && i === count - 1 && !done;
    const ok = CALLS[i][2] || !t.fail;
    el.classList.toggle("on", here && i >= hidden);
    el.classList.toggle("up", here && i < hidden);
    el.style.transitionDelay = t.open ? (i * 35) + "ms" : "0ms";
    const st = el.querySelector(".st"), ini = el.querySelector(".ini");
    const want = !here ? "" : running ? "run" : ok ? "ok" : "bad";
    if (st.dataset.s !== want) {
      st.dataset.s = want;
      st.innerHTML = want === "run" ? '<span class="spin"></span>' : want === "ok" ? `<span class="stamp" style="color:var(--m-ok);display:grid">${I.check}</span>` : want === "bad" ? `<span class="stamp" style="color:var(--m-error);display:grid">${I.x}</span>` : "";
    }
    ini.classList.toggle("wiggle", running);
  });
  const fold = document.getElementById("fold");
  if (!fold) return;
  fold.classList.toggle("on", folded > 0);
  fold.setAttribute("aria-expanded", t.open);
  fold.querySelector(".chev").style.transform = `rotate(${t.open ? 180 : 0}deg)`;
  document.getElementById("odo").style.transform = `translateY(${-16 * folded}px)`;
  const gone = CALLS.slice(0, folded);
  const failed = t.fail ? gone.filter(c => !c[2]).length : 0;
  document.getElementById("plus").textContent = summed ? "" : "+";
  document.getElementById("unit").textContent = (folded === 1 ? "tool" : "tools") + (summed && failed ? `, ${failed} failed` : "") + (S.foldStyle === "names" && !summed ? ":" : "");
  const names = [...new Set(gone.map(c => c[0]))];
  const n = document.getElementById("names");
  n.textContent = S.foldStyle === "names" && !summed ? "\u00a0" + names.slice(0, 3).join(", ") + (names.length > 3 ? ", ..." : "") : "";
  n.style.opacity = t.open ? 0 : 1;
  // One bubble per app, oldest first, the newest on top.
  const apps = [...new Map(gone.map((c, i) => [c[1], i])).entries()].slice(-4);
  const stack = document.getElementById("stack");
  stack.hidden = S.foldStyle !== "icons" && !summed;
  stack.innerHTML = apps.map(([ini, i], k) => `<i style="background:${TINTS[i % 3]};z-index:${k}">${ini}</i>`).join("");
  document.getElementById("thoughts").classList.toggle("on", done);
  document.getElementById("answer").classList.toggle("on", done);
  document.getElementById("answer2").classList.toggle("on", done);
}
