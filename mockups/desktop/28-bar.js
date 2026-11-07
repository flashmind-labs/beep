// ---- Bar ----
// A finished turn: its calls fold into the heading, the chevron opens them.
const dDoneTurn = (turn) => {
  const [ask, steps, reply] = turn, i = LIVE_TURNS.indexOf(turn), open = S.d.ltopen === String(i);
  return dTurn(ask, (steps.length ? dHead(dWorked(3 + 2 * steps.length, steps.length), { act: `dset:ltopen:${open ? "" : i}`, open }) + (open ? dRows(steps.map(([l]) => dTool(l)).join("")) : "") : dHead("Worked for 2s"))
    + `<div class="dcoderen agent md">${reply}</div>`);
};
/// The chat under the dock, as it stands in each live state.
function dLiveChat(o) {
  if (o.chat === "New chat") return `<div class="dbody"><div class="dmuted" style="justify-content:center;padding:22px 0">A new chat. It starts with what you say next.</div></div>`;
  const T = LIVE_TURNS;
  const wrap = inner => `<div class="dbody">${dLog(inner)}</div>`;
  if (S.screen === "dThink" && (S.d.lt === "says" || S.d.lt === "still")) {
    const says = S.d.lt === "says";
    return wrap(dDoneTurn(T[0]) + dDoneTurn(T[1]) + dTurn(T[2][0], `${dWork([["Running code", "run"]], says ? 3 : 24)}<div class="dcoderen agent md">${says ? "Let me check build-box." : "Let me check build-box. Still on it."}</div>`));
  }
  if (o.mode === "think") return wrap(dDoneTurn(T[0]) + dTurn(T[1][0],
    S.d.lt === "tool" ? dWork([["Sending email with Fastmail"], ["Moving an event in Google Calendar", "run"]], 3) : dHead("Working for 1s") + dRows(dThink({ live: true }))));
  if (o.mode === "speak") return wrap(dDoneTurn(T[0]) + dDoneTurn(T[1]));
  return wrap(dDoneTurn(T[0]) + dDoneTurn(T[1]));
}
dLiveView = o => {
  const word = { listen: ["Listening", "on"], think: ["Thinking", ""], speak: ["Speaking", "sp"], idle: ["Listening", "on"] }[o.mode];
  const orb = { listen: "", idle: "idle", think: "think", speak: "speak" }[o.mode];
  const mid = o.mode === "think" || o.mode === "speak";
  const tap = o.mode === "speak" ? 'data-dgo="dInterrupt" data-dact="dset:li:tap"' : o.mode === "think" ? 'data-toast="Beep will not read this answer aloud"' : 'data-dgo="dLeft" data-dact="dset:le:draft"';
  return `<div class="dlv ddock">
    ${o.toast ? `<div class="ltoast" role="status">${o.toast}</div>` : ""}
    <div class="drow1"><button class="dorb ${orb}" ${tap} aria-label="${o.mode === "speak" ? "Stop Beep speaking" : o.mode === "think" ? "Skip the spoken answer" : "Leave live mode"}"><span class="ring b"></span><span class="ring"></span><span class="core"></span></button>
      <div class="dtx"><div class="word ${word[1]}">${o.word || word[0]}</div><div class="cap" aria-live="polite">${o.cap}</div></div>
      <button class="dleave" data-dgo="dLeft" data-dact="dset:le:${mid ? "cancel" : "draft"}" aria-label="${mid ? "Cancel the reply and leave live mode" : "Leave live mode"}">${MINI_X}</button></div>
    <div class="dfoot">${o.soon ? `<div class="dsoon" role="progressbar" aria-label="Sending after the pause"><i></i></div>` : ""}<div class="hint">${o.hint ?? liveHint()}</div></div>
  </div>${o.body ?? dLiveChat(o)}`;
};

// The reply asks first: the approval card sits in the chat as always, answered with a
// click or the usual keys. Live mode waits and goes on once it is answered.
D.dLiveApprove = () => {
  const ask = (S.d.la || "ask") === "ask";
  const card = ask ? `<div class="dcard">
      <div class="head"><span class="dapp">F</span><div style="flex:1;min-width:0;display:grid"><b style="font-size:13.5px">Send email with Fastmail?</b><span style="font-size:12px;color:var(--d-muted)">Fastmail asks first</span></div></div>
      <div class="kv"><span>To</span><span>Alex Chen</span><span>Subject</span><span>Plumber on Thursday</span></div>
      <div class="draft">Hi Alex, Thursday between 9 and 12 works for me. Thanks for sorting it out!</div>
      <div class="acts"><button class="dbtn pri" data-dact="dset:la:done" data-toast="Allowed once">Allow<kbd>⌘${DI.ret}</kbd></button><button class="dbtn" data-dact="dset:la:done" data-toast="Always allowed for Fastmail">Always<kbd>⌥⌘${DI.ret}</kbd></button><button class="dbtn warn" data-dact="dset:la:done" data-toast="Denied">Deny<kbd>⌘D</kbd></button></div>
    </div>` : `<div class="dmuted">${I.shield.replace('width="18" height="18"', 'width="14" height="14"')}Allowed once</div>`;
  const body = `<div class="dbody">${dLog(dDoneTurn(LIVE_TURNS[0]) + dTurn(LIVE_TURNS[1][0], `${dWork(ask ? [["Fastmail: send email", "run"]] : [["Fastmail: send email"], ["Moving an event in Google Calendar", "run"]], ask ? 3 : 9)}
      <div class="dcoderen agent md">Here is the email to Alex. It goes out once you allow it.</div>${card}`))}</div>`;
  const dock = ask
    ? dLiveView({ mode: "idle", word: "Waiting for you", cap: '<span class="ph">Allow or deny in the chat</span>', hint: "Beep goes on once you answer", body })
    : withCancel(dLiveView({ mode: "think", cap: `<span class="prev">${LIVE_TURNS[1][0]}</span><span class="step"><span class="dspin"></span>Moving an event in Google Calendar</span>`, hint: "Option to skip the spoken answer", body }));
  return deskScene(dBar(dock + dHints(ask ? [[`⌘${DI.ret}`, "allow"], [`⌥⌘${DI.ret}`, "always"], ["⌘D", "deny"], ["esc", "leave live"]] : [["tap ⌥", "skip speaking"], ["esc", "cancel reply"]])), { badge: ask });
};
VARIANTS.dLiveApprove = dv("la", [["Asks first", "ask"], ["Allowed", "done"]]);

// Left: the chat stays, with its normal field holding the words and the note on why it ended.
{
  const base = D.dLeft;
  D.dLeft = () => {
    const html = base();
    if (S.d.le === "empty") return html;
    const T = LIVE_TURNS;
    const chat = S.d.le === "cancel"
      ? `<div class="dbody">${dLog(dDoneTurn(T[0]) + dTurn(T[1][0], `${dHead("Worked for 2s")}<div class="dmuted">Stopped<button class="dbtn" data-toast="Sent again">Retry</button></div>`))}</div>`
      : `<div class="dbody">${dLog(dDoneTurn(T[0]) + dDoneTurn(T[1]))}</div>`;
    const at = html.lastIndexOf('<div class="dhints">');
    return at < 0 ? html : html.slice(0, at) + chat + html.slice(at);
  };
}

{
  const g = DESK_SCREENS.find(g => g.group === "Live voice").items;
  const name = { dLive: "LiveDock over the chat, listening", dThink: "LiveDock, thinking", dSpeak: "LiveDock, speaking", dInterrupt: "LiveDock, tap or “stop”", dCommands: "LiveDock, listen::Command", dLeft: "PromptBar, draft, chat kept" };
  g.forEach(it => { if (name[it[0]]) it[2] = name[it[0]]; });
  g.splice(g.findIndex(it => it[0] === "dInterrupt"), 0, ["dLiveApprove", "Asks first", "LiveDock, approval card in the chat"]);
}
