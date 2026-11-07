// ---- Skills ----
// How the user, or a group, likes a recurring task done. Beep writes them on its own after a chat has had
// enough turns, or after a turn with many tool calls, and rewrites one when the user corrects it.
// Personal skills load only in the user's chats, a group's only in that group.
Object.assign(S, { skills: "list" });
const skillIc = '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M5 19l9-9"/><path d="M14 4l1.2 2.8L18 8l-2.8 1.2L14 12l-1.2-2.8L10 8l2.8-1.2z"/><path d="M19 13l.6 1.4L21 15l-1.4.6L19 17l-.6-1.4L17 15l1.4-.6z"/></svg>';
const SKILLS = [
  { name: "Book a padel court", desc: "Booking a court at the club for an evening game", uses: 6, when: "Used yesterday", by: "Beep", on: true,
    body: ["Open the club's booking page with the browser and sign in with the saved account.", "Look for 19:00 first, then 20:00. Never earlier on weekdays.", "Pick an indoor court when rain is forecast.", "Book for four, then post the court and time in the Padel group."],
    prefs: ["Court 3 or 4 when free", "Pay with the club credit, not the card"] },
  { name: "Weekly spending summary", desc: "Summing up the week's card spending from the bank emails", uses: 3, when: "Used Sunday", by: "you", on: true,
    body: ["Search Gmail for the bank's card notifications since Monday.", "Group by shop type: food, transport, home, other.", "Show a short table, totals first, then the three biggest payments."],
    prefs: ["Euros, no decimals", "Leave out transfers between own accounts"] },
  { name: "Plan a weekend trip", desc: "Finding trains and a place to stay for a weekend away", uses: 2, when: "Used Oct 2", by: "Beep", on: false,
    body: ["Ask where and which weekend if not said.", "Search trains leaving Friday after 17:00 and back Sunday evening.", "Find two or three places to stay near the station, under 120 a night."],
    prefs: ["Window seat", "No hostels"] },
];
const GROUP_SKILLS = [
  { group: "Tokyo trip", name: "Split a dinner bill", desc: "Working out who owes what after a group dinner", uses: 4, when: "Used Oct 8", by: "Beep", on: true },
];
function skillCard(k, i, group) {
  return `<button class="card" style="display:flex;gap:12px;align-items:flex-start;text-align:left;width:100%;cursor:pointer${k.on ? "" : ";opacity:.65"}" data-set="skills:${group ? "group" : "detail" + i}">
    <span class="grow"><b style="font-weight:600;font-size:14px">${k.name}</b><span class="muted" style="font-size:12px;line-height:17px">${k.desc}</span>
      <span style="font-size:12px;color:var(--m-dim);margin-top:2px">${k.on ? `${k.when} · ${k.uses} uses` : "Paused"}${k.by === "you" ? " · edited by you" : ""}</span></span>
    <span style="color:var(--m-muted);margin-top:2px">${I.chev}</span></button>`;
}
R.skills = () => {
  const st = S.skills;
  if (st.startsWith("detail") || st === "group" || st === "edit" || st === "delete") {
    const k = st === "group" ? { ...GROUP_SKILLS[0], body: ["Ask who was at the dinner if it is not clear from the chat.", "Split shared dishes evenly and own drinks by who had them.", "Post one line per person with what they owe and to whom."], prefs: ["Round to whole euros", "Dani pays by Bizum, the rest by bank transfer"] }
      : SKILLS[+(st.replace("detail", "")) || 0];
    const top = `<div class="bar" style="padding-left:6px"><button class="ib" data-set="skills:list" aria-label="Back">${I.back}</button><div class="title" style="flex:1;font-size:20px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis">${k.name}</div>
      <button class="ib" data-set="skills:edit" aria-label="Edit">${I.edit}</button><button class="ib" style="color:var(--m-error)" data-set="skills:delete" aria-label="Delete">${I.trash}</button></div>`;
    let out = top + `<div class="body" style="gap:12px">
      ${k.group ? `<span class="tag" style="align-self:flex-start;background:var(--m-t2)">${k.group}</span>` : ""}
      <div class="muted" style="font-size:14px;line-height:20px">${k.desc}</div>
      <div class="card row" style="align-items:center"><span class="grow"><b style="font-weight:500;font-size:14px">Use this skill</b>
        <span class="muted" style="font-size:12px">${k.on ? `${k.when} · ${k.uses} uses` : "Paused: Beep works it out each time"}</span></span>
        <button class="switch" role="switch" aria-checked="${k.on}" aria-label="Use this skill" data-toast="${k.on ? "Paused" : "Resumed"}"></button></div>
      <div class="card" style="display:grid;gap:8px">
        <div class="label">Steps</div>
        <ol style="margin:0;padding-left:20px;display:grid;gap:4px;font-size:14px;line-height:20px">${k.body.map(b => `<li>${b}</li>`).join("")}</ol>
        <div class="label" style="margin-top:4px">Preferences</div>
        <ul style="margin:0;padding-left:20px;display:grid;gap:2px;font-size:14px;line-height:20px">${k.prefs.map(b => `<li>${b}</li>`).join("")}</ul>
      </div>
      <div class="banner" style="background:var(--m-t1)">${skillIc}<span style="flex:1">${k.by === "you" ? "You edited this on Oct 9." : "Beep updated this yesterday, after you asked for court 3."}</span>
        <button class="text-btn" style="min-height:32px;padding:0 8px" data-toast="Back to the earlier version">Undo</button></div>
    </div>`;
    if (st === "edit") out += `<div class="dialog-wrap"><div class="dialog"><h3>Edit skill</h3>
      <label class="fl">Name<input class="field" value="${esc(k.name)}"></label>
      <label class="fl">When it applies<input class="field" value="${esc(k.desc)}"></label>
      <label class="fl">Instructions<textarea class="field" rows="7" style="font-size:13px;line-height:19px">${k.body.map((b, i) => `${i + 1}. ${b}`).join("\n")}\n\nPreferences:\n${k.prefs.map(p => `- ${p}`).join("\n")}</textarea></label>
      <div class="end"><button class="text-btn" data-set="skills:detail0">Cancel</button><button class="go" data-set="skills:detail0" data-toast="Saved">Save</button></div></div></div>`;
    if (st === "delete") out += `<div class="dialog-wrap"><div class="dialog"><h3>Delete this skill?</h3>
      <div class="muted">Beep forgets how you like "${k.name}" done. It may learn it again if you keep asking for it.</div>
      <div class="end"><button class="text-btn" data-set="skills:detail0">Cancel</button><button class="go" style="background:var(--m-error);color:#fff" data-set="skills:list" data-toast="Skill deleted">Delete</button></div></div></div>`;
    return out;
  }
  let out = back("Skills") + `<div class="body" style="gap:8px">`;
  if (st === "empty") return out + `<div class="empty">No skills yet.<br>When Beep has to work out how you like something done, like booking a court or a weekly summary, it writes it down here and does it your way next time.</div></div>`;
  out += `<div class="muted" style="font-size:13px;line-height:18px;padding:0 4px">Beep writes these from how you ask for things, and updates them when you correct it.</div>
    <div class="label" style="padding:8px 4px 0">Yours</div>` + SKILLS.map((k, i) => skillCard(k, i, false)).join("")
    + `<div class="label" style="padding:8px 4px 0">${GROUP_SKILLS[0].group}</div>` + GROUP_SKILLS.map((k, i) => skillCard(k, i, true)).join("");
  return out + `</div>`;
};
VARIANTS.skills = vs("skills", [["List", "list"], ["Skill", "detail0"], ["Edited by you", "detail1"], ["Paused", "detail2"], ["Group's", "group"], ["Editing", "edit"], ["Delete", "delete"], ["Empty", "empty"]]);
I_GO.skills = "iSkills";
IOS_SCREENS.find(g => g.group === "Settings").items.splice(1, 0, ["iSkills", "Skills", "SkillsView"]);
P.iSkills = () => iGroup(R.skills()); VARIANTS.iSkills = VARIANTS.skills;
