// ---- Desktop live voice ----
// One live view in the bar: an orb that follows the microphone while
// listening and Beep's voice while speaking, one status word, and captions.
// Words come from the server's realtime transcription, as they are said.
(() => {
  const st = document.createElement("style");
  st.textContent = ``;
  document.head.append(st);
})();
const LIVE_TURNS = [
  ["Did the plumber get back to me?", [], "Yes, Alex wrote at 08:12: the plumber can come Thursday between 9 and 12, and she asks if that works."],
  ["Tell Alex Thursday works, and move the dentist to next week", [["Sent an email with Fastmail", "F"], ["Moved an event in Google Calendar", "G"]],
    "Sent Alex a yes for Thursday 9 to 12. The dentist is moved to next Friday at 17:15, and I emailed the clinic to confirm."],
  ["Is build-box running out of disk?", [["Ran code", "S"]], "The root disk is 91% full, 8 GB left. Most of it is Docker images."],
  ["Clear the dangling images then", [["Ran code", "S"]], "Done, that freed 23 GB. The root disk is at 58% now."],
  ["Remind me to call the plumber tomorrow at nine", [["Created a reminder", "R"]], "Done. I will remind you tomorrow at 09:00."],
  ["Draft a thank-you to Alex for sorting the plumber", [], "Here is a draft: “Thanks for arranging Thursday, Alex. 9 to 12 works for me.” Send it?"],
];
P.iLiveChat = () => iChatBar().replace(">New conversation<", ">Plumber and dentist<") + `<div class="body" style="padding:8px 18px 16px;gap:18px">
    ${LIVE_TURNS.map(([ask, steps, reply]) => `<div class="user" style="border-width:2px;box-shadow:3px 3px 0 var(--m-ledge);border-radius:var(--radius) 18px 6px 18px;margin-left:48px">${ask}</div>`
      + (steps.length ? `<div style="display:flex;flex-direction:column;gap:6px">${steps.map(([l, ini], i) => iChip(l, ini, "ok", i)).join("")}</div>` : "")
      + `<div class="reply">${reply}</div>`).join("\n    ")}
  </div>` + iComposer();
IOS_SCREENS.find(g => g.group === "Chat").items.splice(2, 0, ["iLiveChat", "Chat after live mode", "ChatView"]);
