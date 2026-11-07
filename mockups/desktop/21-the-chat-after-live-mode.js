// ---- The chat after live mode ----
// Spoken turns are ordinary turns: they show exactly like typed ones, with their steps
// folded under the prompt as usual. Only the chat list marks a chat that had live mode.
const LT_FIRST = "You: Tell Alex Thursday works, and move the dentist to next week";
const LIVE_TURNS = [
  ["Did the plumber get back to me?", [], "Yes, Alex wrote at 08:12: the plumber can come Thursday between 9 and 12, and she asks if that works."],
  ["Tell Alex Thursday works, and move the dentist to next week", [["Sent an email with Fastmail", "F"], ["Moved an event in Google Calendar", "G"]],
    "Sent Alex a yes for Thursday 9 to 12. The dentist is moved to next Friday at 17:15, and I emailed the clinic to confirm."],
  ["Is build-box running out of disk?", [["Ran code", "S"]], "The root disk is 91% full, 8 GB left. Most of it is Docker images."],
  ["Clear the dangling images then", [["Ran code", "S"]], "Done, that freed 23 GB. The root disk is at 58% now."],
  ["Remind me to call the plumber tomorrow at nine", [["Created a reminder", "R"]], "Done. I will remind you tomorrow at 09:00."],
  ["Draft a thank-you to Alex for sorting the plumber", [], "Here is a draft: “Thanks for arranging Thursday, Alex. 9 to 12 works for me.” Send it?"],
];
(() => {
  const st = document.createElement("style");
  st.textContent = `.mac .dchat .sub { color: var(--d-dim); font-weight: 400; margin-left: 8px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
.mac .dchat .vmic { display: inline-flex; margin-right: 6px; color: var(--d-emerald); vertical-align: -1px }`;
  document.head.append(st);
})();
// Desktop: the same chat, every turn in the usual look.
D.dSpokenChat = () => {
  return deskScene(dBar(D_ASK + `<div class="dbody">
      ${dLog(LIVE_TURNS.map(dDoneTurn).join(""))}
    </div>` + dFollow + dHints([["⌥ ⌥ ⌥", "live mode"], ...DHINTS.slice(0, 2), ["esc", "to hide"]])));
};
DESK_SCREENS.find(g => g.group === "Live voice").items.forEach(it => { if (it[0] === "dSpokenChat") { it[1] = "Chat after live mode"; it[2] = "PromptBar"; } });
DESK_SCREENS.find(g => g.group === "Live voice").items.push(["dLiveChats", "Chat list with a live chat", "ChatPicker, spoken"]);
D.dLiveChats = () => {
  const prev = S.d.chats; S.d.chats = "list";
  let html = D.dChats();
  S.d.chats = prev;
  const row = `<button class="dchat" role="option" aria-selected="false">${MIC_GLYPH}<span class="t">Plumber and dentist<span class="sub">${LT_FIRST}</span></span><span class="w">14:07</span></button>`;
  return html.replace('<div class="dlist" role="listbox" aria-label="Chats">', '<div class="dlist" role="listbox" aria-label="Chats">' + row);
};
// Thinking: Beep says what it is about to do (the model's own line, or a fallback
// when a tool starts after 2 s of silence), and "Still on it" once after 10 s.
VARIANTS.dThink = dv("lt", [["Sent", "think"], ["Using a tool", "tool"], ["Says what it is doing", "says"], ["Still on it", "still"]]);
const thinkBase = D.dThink;
D.dThink = () => {
  const v = S.d.lt;
  if (v !== "says" && v !== "still") return thinkBase();
  const said = v === "says" ? "Let me check build-box." : "Still on it.";
  const step = "Running code";
  return deskScene(dBar(dLiveView({ mode: "speak", cap: `<span class="prev">Is build-box running out of disk?</span>${said}<span style="display:flex;justify-content:center;margin-top:6px"><span class="step"><span class="dspin"></span>${step}</span></span>`, chat: "Plumber and dentist", hint: "Beep reads the answer when it is ready · Option to stop" })
    + dHints([["tap ⌥", "stop speaking"], ["esc", "leave live"]])));
};
