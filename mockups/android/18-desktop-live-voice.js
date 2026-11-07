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
// Phones open the same chats as ordinary turns.
R.liveChat = () => topBar().replace("Boiler and Saturday weather", "Plumber and dentist") + `<div class="body">
    <div style="align-self:stretch;text-align:center;font-size:11px;color:var(--m-dim)">Today</div>
    ${LIVE_TURNS.map(([ask, steps, reply]) => `<div class="user">${ask}</div>${steps.map(([l, ini]) => toolChip(l, ini)).join("")}<div class="reply">${reply}</div>`).join("\n    ")}
  </div>` + composer();
SCREENS.find(g => g.group === "Chat").items.splice(2, 0, ["liveChat", "Chat after live mode", "ChatScreen"]);
