// ---- Desktop live voice ----
// One live view in the bar: an orb that follows the microphone while
// listening and Beep's voice while speaking, one status word, and captions.
// Words come from the server's realtime transcription, as they are said.
(() => {
  const st = document.createElement("style");
  st.textContent = `.mac .dlv { flex: none; display: grid; justify-items: center; gap: 6px; padding: 18px 24px 14px; position: relative }
.mac .dlv .top { position: absolute; left: 14px; right: 14px; top: 12px; display: flex; align-items: center; gap: 8px; font-size: 12px; color: var(--d-muted) }
.mac .dlv .top .dlogo { width: 20px; height: 20px; font-size: 11px; border-radius: 6px }
.mac .dlv .top .live { display: inline-flex; align-items: center; gap: 6px; margin-left: auto; font-weight: 600; font-size: 12px; color: var(--d-emerald) }
.mac .dlv .top .live i { width: 6px; height: 6px; border-radius: 50%; background: currentColor }
.mac .dorb { --lvl: 0; --c1: #A6EBCF; --c2: #5E6AD2; --c3: #0F3B2C; --glow: 59,155,122; width: 116px; height: 116px; margin-top: 10px; position: relative; display: grid; place-items: center }
.mac .dorb.speak { --c1: #FFD0C7; --c2: #F2994A; --c3: #5A231C; --glow: 240,122,106 }
.mac .dorb.think { --c1: #7FBFA6; --c2: #4C56B0; --c3: #0D2A20; --glow: 59,155,122 }
.mac .dorb.idle { --c1: #6E8C7F; --c2: #2A2B2E; --c3: #151617; --glow: 146,165,154 }
.mac .dorb .core { width: 76px; height: 76px; border-radius: 50%; background: radial-gradient(circle at 36% 30%, var(--c1) 0%, var(--c2) 46%, var(--c3) 100%);
  transform: scale(calc(1 + var(--lvl) * .22)); box-shadow: 0 0 calc(18px + var(--lvl) * 44px) rgba(var(--glow), calc(.28 + var(--lvl) * .4)), inset 0 -6px 14px rgba(0,0,0,.35); transition: transform .08s linear, box-shadow .08s linear }
.mac .dorb .ring { position: absolute; inset: 20px; border-radius: 50%; border: 1.5px solid rgba(var(--glow), calc(.12 + var(--lvl) * .35)); transform: scale(calc(1.05 + var(--lvl) * .42)); transition: transform .12s linear }
.mac .dorb .ring.b { transform: scale(calc(1.18 + var(--lvl) * .7)); border-color: rgba(var(--glow), calc(.05 + var(--lvl) * .2)) }
.mac .dorb.think .core { animation: none}
.mac .dorb.think .ring { animation: none}
@keyframes dbreath { 0%,100% { transform: scale(.94) } 50% { transform: scale(1.06) } }
.mac .dlv .word { font-size: 12px; font-weight: 600; color: var(--d-muted); margin-top: 2px }
.mac .dlv .word.on { color: var(--d-emerald) } .mac .dlv .word.sp { color: var(--d-coral) }
.mac .dlv .cap { width: 100%; max-width: 560px; min-height: 52px; text-align: center; font-size: 19px; line-height: 26px; text-wrap: balance; color: var(--d-fg) }
.mac .dlv .cap .tent { color: var(--d-muted) }
.mac .dlv .cap .prev { display: block; font-size: 14px; line-height: 20px; color: var(--d-dim); margin-bottom: 2px }
.mac .dlv .cap .ph { color: var(--d-dim) }
.mac .dlv .cap .cmd { color: var(--d-emerald); font-weight: 600 } .mac .dlv .cap .cmd.no { color: var(--d-coral) }
.mac .dlv .cap .gone { color: var(--d-dim); text-decoration: line-through }
.mac .dlv .step { display: inline-flex; align-items: center; gap: 8px; font-size: 14px; color: var(--d-muted) }
.mac .dlv .hint { font-size: 12px; color: var(--d-dim); text-align: center }
.mac .dlv .hint kbd { font-size: 10.5px }
.mac .dlv .ltoast { position: absolute; top: 12px; left: 50%; transform: translateX(-50%); padding: 3px 10px; border-radius: 999px; background: var(--d-elev); border: 1px solid var(--d-border); font-size: 12px; font-weight: 500; color: var(--d-fg); white-space: nowrap; animation: barIn .22s var(--ease) }
.mac .dsoon { width: 140px; height: 3px; border-radius: 2px; background: var(--d-border); overflow: hidden }
.mac .dsoon i { display: block; height: 100%; background: var(--d-emerald); transform-origin: left; animation: none}
@keyframes dsoon { from { transform: scaleX(1) } to { transform: scaleX(0) } }
.mac .dsays { flex: none; display: flex; flex-wrap: wrap; gap: 6px 14px; justify-content: center; padding: 10px 16px 12px; border-top: 1px solid var(--d-border); font-size: 12px; color: var(--d-muted) }
.mac .dsays b { font-weight: 500; color: var(--d-fg) }
.mac .dask .vmic { display: inline-flex; vertical-align: -1px; margin-right: 6px; color: var(--d-muted) }
.mac .dask.gone { color: var(--d-muted) }
@media (prefers-reduced-motion: reduce) { .mac .dorb .core, .mac .dorb .ring { transition: none; transform: none !important } }`;
  document.head.append(st);
})();

// Only two spoken commands: "send", and "stop", which sends too while the user talks.
const LIVE_SAYS = [["send", "sends now"], ["stop", "sends, or stops Beep"]];
const dOrb = (cls = "") => `<div class="dorb ${cls}" aria-hidden="true"><span class="ring b"></span><span class="ring"></span><span class="core"></span></div>`;
const liveHint = () => S.d.vsend === "word" ? "Say “send” or “stop” to send · Option to stop" : "Pause to send · Option to stop";
/// The live view: orb, status word, caption, hint. `o.toast` flashes a recognized command.
function dLiveView(o) {
  const word = { listen: ["Listening", "on"], think: ["Thinking", ""], speak: ["Speaking", "sp"], idle: ["Listening", "on"] }[o.mode];
  const orb = { listen: "", idle: "idle", think: "think", speak: "speak" }[o.mode];
  return `<div class="dlv">
    <div class="top"><span class="dlogo" style="background:#0B0E1D;color:#fff">${DI.tray}</span>${o.chat || "New chat"}<span class="live"><i></i>Live</span></div>
    ${o.toast ? `<div class="ltoast" role="status">${o.toast}</div>` : ""}
    ${dOrb(orb)}
    <div class="word ${word[1]}">${word[0]}</div>
    <div class="cap" aria-live="polite">${o.cap}</div>
    ${o.soon ? `<div class="dsoon" role="progressbar" aria-label="Sending after the pause"><i></i></div>` : ""}<div class="hint">${o.hint ?? liveHint()}</div>
  </div>`;
}
const LIVE_KEYS = [["tap ⌥", "leave"], ["hold ⌥", "talk, let go to send"], ["esc", "leave live"], ["⌘/", "what to say"]];
const LIVE_REPLY = ["Sent Alex a yes for Thursday 9 to 12.", "The dentist is moved to next Friday at 17:15,", "and I emailed the clinic to confirm."];
const VN_TITLE = "Inbox sweep";
const VN_LINES = ["Two emails need a reply today.", "Alex asks if Thursday 9 to 12 works for the plumber,", "and the dentist wants you to confirm Friday at 17:15."];
const MIC_GLYPH = `<span class="vmic" aria-label="Spoken">${small(I.micS, 12)}</span>`;
// A turn in the bar: "You" and the prompt, then the work and the answer (wrap turns in dLog).
const dTurn = (ask, body, id) => dYou(ask, id) + body;
const dCode = t => `<code>${esc(t)}</code>`;
// A turn's work: the heading counts the calls, the chevron folds the rows into it.
// A finished turn's work, folded into its heading; the chevron opens the calls.
const dFold = (steps, secs = 6) => dHead(dWorked(secs, steps.length), { act: "steps", open: !!S.d.steps }) + (S.d.steps ? dRows(steps.map(l => dTool(l)).join("")) : "");
const dDone = (steps, secs = 6) => dHead(dWorked(secs, steps.length), { act: `dset:wfold:${S.d.wfold ? "" : 1}`, open: !S.d.wfold }) + dRows(steps.map(([l, s]) => dTool(l, s)).join(""), !!S.d.wfold);
const dWork = (steps, secs = 4) => dHead(`Working for ${dDur(secs)} · ${steps.length} tool ${steps.length === 1 ? "call" : "calls"}`, { act: `dset:wfold:${S.d.wfold ? "" : 1}`, open: !S.d.wfold }) + dRows(steps.map(([l, s]) => dTool(l, s)).join(""), !!S.d.wfold) + dLive("Working", dDur(secs));
// The field once the prompt is in the log.
const dAskF = (extra = "") => dPrompt('<span style="color:var(--d-dim)">Ask a follow-up…</span>', extra);
Object.assign(S.d, { shell: "done", agent: "ask", ctl: "done", lv: "words", lt: "think", ls: "reply", li: "tap", lc: "cancel", le: "draft", vsend: "pause" });

// The orb follows a level: the microphone while listening, the playback while speaking.
// Here it is simulated: bursts like speech, smoother for Beep's voice.
let orbRaf = 0;
function orbLoop() {
  cancelAnimationFrame(orbRaf);
  if (matchMedia("(prefers-reduced-motion: reduce)").matches) return;
  const t0 = performance.now();
  const tick = now => {
    const orbs = document.querySelectorAll("#desk .dorb:not(.think)");
    if (!orbs.length) return;
    const t = (now - t0) / 1000;
    orbs.forEach(o => {
      let lvl;
      if (o.classList.contains("idle")) lvl = 0.04 + 0.03 * Math.sin(t * 1.6);
      else if (o.classList.contains("speak")) lvl = Math.max(0, 0.35 + 0.3 * Math.sin(t * 7.3) * Math.sin(t * 2.1) + 0.12 * Math.sin(t * 17));
      else { const burst = Math.max(0, Math.sin(t * 1.3)) ** 2; lvl = burst * (0.45 + 0.35 * Math.abs(Math.sin(t * 9.1))) + 0.04; }
      o.style.setProperty("--lvl", Math.min(1, lvl).toFixed(3));
    });
    orbRaf = requestAnimationFrame(tick);
  };
  orbRaf = requestAnimationFrame(tick);
}
// Beep's caption moves on a sentence at a time, roughly with the audio.
let capTimer = 0;
function captionLoop() {
  clearInterval(capTimer);
  const cap = document.querySelector("#desk .dlv .cap[data-lines]");
  if (!cap) return;
  const lines = JSON.parse(cap.dataset.lines);
  let i = 0;
  const draw = () => { cap.innerHTML = `${i ? `<span class="prev">${lines[i - 1]}</span>` : ""}${lines[i]}`; };
  draw();
  capTimer = setInterval(() => { if (!cap.isConnected) return clearInterval(capTimer); i = (i + 1) % lines.length; draw(); }, 3200);
}

Object.assign(D, {
  // Listening: the words stream in as they are said. Pauses never send.
  dLive: () => {
    const v = S.d.lv;
    if (v === "sheet") return deskScene(dBar(`<div class="dover lsheet" role="dialog" aria-label="What to say"><div class="vt"><span class="tt"><b>What to say</b></span></div>${dKeyGroups([["Say", LIVE_SAY], ["Keys", LIVE_TALK]])}${dHints([["esc", "close"], ["⌘/", "close"]])}</div>`));
    if (v === "off") return deskScene(dBar(dLiveView({ mode: "idle", cap: `<span class="ph">Voice is not available right now. Try again in a moment.</span>`, hint: "Say “send” or “stop” to send · tap ⌥ to leave" })
      + dHints([["esc", "leave live"]])));
    const cap = {
      words: `Remind me to call the plumber tomorrow at nine <span class="tent">and ask</span>`,
      pause: `Remind me to call the plumber tomorrow at nine, and ask Sam if Saturday still works`,
      soon: `Remind me to call the plumber tomorrow at nine, and ask Sam if Saturday still works`,
      empty: `<span class="ph">Start talking…</span>`,
      hold: `What's the weather like in Girona <span class="tent">this weekend</span>`,
      send: `Remind me to call the plumber tomorrow at nine <span class="cmd">send</span>`,
    }[v];
    const hint = { pause: "Say “send” or “stop” to send · tap ⌥ to leave", soon: "Pause to send · tap ⌥ to leave", hold: "Let go of Option to send", send: "Sending…" }[v];
    return deskScene(dBar(dLiveView({ mode: v === "empty" ? "idle" : "listen", cap, hint, soon: v === "soon" }) + dHints(v === "hold" ? [["let go of ⌥", "send"], ["any key", "cancel"]] : LIVE_KEYS)));
  },
  // Thinking: the message went; the current step shows while Beep works.
  dThink: () => {
    const tool = S.d.lt === "tool";
    return deskScene(dBar(dLiveView({ mode: "think", cap: tool ? `<span class="step"><span class="dspin"></span>Moving an event in Google Calendar</span>` : `<span class="prev">Tell Alex Thursday works, and move the dentist to next week</span>`, chat: "Plumber and dentist", hint: "Option to skip the spoken answer" })
      + dHints([["tap ⌥", "skip speaking"], ["esc", "cancel reply"]])));
  },
  // Speaking: the current sentence of the reply, moving on with the audio.
  // A voice note opens the live view here, without taking focus.
  dSpeak: () => {
    const note = S.d.ls === "note";
    const lines = note ? VN_LINES : LIVE_REPLY;
    return deskScene(dBar(dLiveView({ mode: "speak", cap: `<span class="prev">${lines[0]}</span>${lines[1]}`, chat: note ? VN_TITLE : "Plumber and dentist", hint: "Say “stop” or tap Option to talk" }).replace('class="cap"', `class="cap" data-lines='${JSON.stringify(lines)}'`)
      + dHints(note ? [["tap ⌥", "stop"], ["⌥ ⌥", "open the chat"], ["esc", "dismiss"]] : [["tap ⌥", "interrupt"], ["hold ⌥", "reply"], ["esc", "cancel reply"]])));
  },
  // Tapping Option or saying "stop" while Beep speaks cuts it off and listens.
  dInterrupt: () => {
    const said = S.d.li === "said";
    return deskScene(dBar(dLiveView({ mode: "listen", toast: said ? "Stopped" : "Stopped speaking", chat: "Plumber and dentist",
      cap: `<span class="prev">The dentist is moved to next Friday at 17:15,</span>${said ? `<span class="cmd">stop</span>` : `<span class="ph">Start talking…</span>`}` }) + dHints(LIVE_KEYS)));
  },
  // Spoken commands, heard on the live transcript. Each is a whole phrase on its own;
  // only "send" ends a sentence. They act here and never reach Beep as text.
  dCommands: () => {
    const c = S.d.lc;
    const v = {
      stop: { toast: "Sending", cap: `Text Sam that I'm running late <span class="cmd">stop</span>`, hint: "“stop” sends, as “send” does" },
      normal: { cap: `Stop the server on build-box <span class="tent">and</span>`, hint: "“stop” inside a sentence is just a word" },
    }[c];
    return deskScene(dBar(dLiveView({ mode: v.mode || "listen", toast: v.toast, cap: v.cap, hint: v.hint, chat: "Plumber and dentist" })
      + `<div class="dsays">${LIVE_SAYS.map(([k, t]) => `<span><b>“${k}”</b> ${t}</span>`).join("")}</div>` + dHints(LIVE_KEYS)));
  },
  // Leaving live mode: Escape, a tap of Option while listening, "bye", or 2 minutes quiet.
  // Words said and not sent stay as a draft in the chat's field.
  dLeft: () => {
    const e = S.d.le;
    if (e === "empty") return deskScene(dBar(dPrompt('<span class="ph" style="color:var(--d-dim)">Ask a follow-up…</span>') + `<div class="dbody">
      ${dLog(dTurn(MIC_GLYPH + "What's on tomorrow?", `${dHead(dWorked(3, 1))}<div class="dcoderen agent md">Two things: the plumber between 9 and 12, and the dentist at 17:15.</div>`))}</div>` + dHints()));
    const note = { draft: "Left live mode. Your words are kept here.", quiet: "Live mode ended after 2 minutes quiet. Your words are kept here.", bye: "Said “bye”. Your words are kept here." }[e];
    return deskScene(dBar(`<div class="dinput multi dgrip"><span class="txt">Remind me to call the plumber tomorrow at nine, and ask Sam if Saturday still works<span class="caret" style="height:20px"></span></span></div>
      <div class="dmuted" style="padding:0 16px 10px">${I.micS}${note}</div>` + dHints([[DI.ret, "send"], ["⌥ ⌥ ⌥", "talk again"], ["esc", "to hide"]])));
  },
  // Settings, General: voice notes and how a spoken message ends.
  dLiveSettings: () => {
    const word = S.d.vsend === "word";
    return deskScene(dBar(`<div class="dhead"><span class="tt"><b>Settings</b><small>Beep for macOS 0.2.0</small></span></div>
      <div class="dform" style="padding-top:12px">
        <div class="dsec" style="padding:6px 0 0">General</div>
        <div class="dtoggle"><span><b>Voice notes</b><small>Speak new notices when you are at this Mac and not on a call, then keep listening. Tap Option three times to talk live.</small></span><button class="dswitch" role="switch" aria-checked="true" aria-label="Voice notes"></button></div>
        <div class="dtoggle"><span><b>When I stop talking</b><small>${word ? "Pauses never send. Say “send” or “stop” when you are done." : "Sends after a short pause. Saying “send” sends at once."}</small></span></div>
        <div class="dchips" role="radiogroup" aria-label="When I stop talking" style="margin-top:-6px"><button class="dchip" role="radio" aria-pressed="${!word}" aria-checked="${!word}" data-dact="dset:vsend:pause">Send when I stop talking</button><button class="dchip" role="radio" aria-pressed="${word}" aria-checked="${word}" data-dact="dset:vsend:word">Wait until I say “send”</button></div>
      </div>` + dHints([["esc", "close"]])));
  },
  // The chat afterwards: plain text of both sides, a mic on what was said aloud.
  dSpokenChat: () => deskScene(dBar(D_ASK + `<div class="dbody">
      <div class="dturn"><div style="display:grid;gap:2px"><span style="font-size:11.5px;font-weight:600;color:var(--d-muted);display:flex;gap:6px;align-items:center">${I.speaker}${VN_TITLE}</span><span>${VN_LINES.join(" ")}</span></div></div>
      ${dTurn(MIC_GLYPH + "Tell Alex Thursday works, and move the dentist to next week", `${dDone([["Sending email with Fastmail"], ["Reading calendar"], ["Moving an event in Google Calendar"]])}<div>${LIVE_REPLY.join(" ")}</div>`)}
      ${dTurn(MIC_GLYPH + "Remind me to call the plumber tomorrow at nine", `${dFold(["Creating a reminder"])}<div class="dcoderen agent md">Done. I will remind you <b>tomorrow at 09:00</b> to call the plumber.</div>`)}
    </div>` + dFollow + dHints([["⌥ ⌥ ⌥", "live mode"], ...DHINTS.slice(0, 2), ["esc", "to hide"]]))),
});
