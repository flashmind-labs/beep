// ---- Desktop: Skills ----
// The phone's Skills in the bar. Rows are grouped by owner, the user's own first, then each group's.
// Space pauses or resumes, Tab shows the row's actions, Return opens the skill.
D.dSkills = () => {
  const st = S.d.sk;
  const all = [...SKILLS.map((k, i) => ({ ...k, i })), ...GROUP_SKILLS.map(k => ({ ...k, i: 9 }))];
  const sel = 0, k = SKILLS[0];
  const paused = all.filter(x => !x.on).length;
  const sic = skillIc.replace('width="20" height="20"', 'width="16" height="16"');
  const head = (extra = "") => `<div class="dhead"><span class="tt"><b>Skills</b><small>${st === "empty" ? "None yet" : `${all.length} skills, ${paused} paused`}</small></span>${extra}</div>`;
  if (st === "empty") return deskScene(dBar(head() + `<div style="padding:28px 24px 32px;display:grid;justify-items:center;gap:8px;text-align:center"><span style="color:var(--d-dim);display:grid">${skillIc}</span>
    <b style="font-size:14px">No skills yet.</b><span class="dmuted" style="display:block;max-width:44ch">Beep saves how you like things done here.</span></div>` + dHints([["esc", "back"]])));
  if (st === "view" || st === "edit" || st === "delete") {
    const top = `<div class="dhead"><span class="tt"><b>${k.name}</b><small>${k.desc}</small></span>
      <button class="dswitch" role="switch" aria-checked="${k.on}" aria-label="Use this skill" data-toast="${k.on ? "Paused" : "Resumed"}"></button>
      <button class="dbtn" data-dact="sk:edit">Edit<kbd>⌘${DI.ret}</kbd></button><button class="dbtn warn" data-dact="sk:delete">Delete<kbd>⌘⌫</kbd></button></div>`;
    const md = `${k.body.map((b, i) => `${i + 1}. ${b}`).join("\n")}\n\nPreferences:\n${k.prefs.map(p => `- ${p}`).join("\n")}`;
    if (st === "edit") return deskScene(dBar(`<div class="dhead"><span class="tt"><b>Edit skill</b><small>Beep keeps the old version, so you can undo</small></span></div>
      <div class="dform" style="padding-top:12px;flex:1;min-height:0;overflow:auto;scrollbar-width:none">
        <label class="dfield"><span>Name</span><span class="f">${k.name}</span></label>
        <label class="dfield"><span>When it applies</span><span class="f">${k.desc}</span></label>
        <label class="dfield on"><span>Instructions</span><span class="f area" style="white-space:pre-wrap;font-size:13px;line-height:19px">${md}</span></label>
        <div style="display:flex;gap:8px"><button class="dbtn pri" data-dact="sk:view" data-toast="Saved">Save<kbd>⌘${DI.ret}</kbd></button><button class="dbtn quiet" data-dact="sk:view">Cancel</button></div>
      </div>` + dHints([["tab", "next field"], [`⌘${DI.ret}`, "save"], ["esc", "cancel"]])));
    const confirm = st === "delete" ? `<div class="dcard" style="margin:12px 16px 0"><div style="padding:10px 12px;display:grid;gap:8px">
        <b style="font-size:13.5px">Delete this skill?</b><div class="dmuted" style="display:block">Beep forgets how you like "${k.name}" done. It may learn it again if you keep asking for it.</div>
        <div style="display:flex;gap:8px"><button class="dbtn warn" data-dact="sk:list" data-toast="Skill deleted">Delete<kbd>${DI.ret}</kbd></button><button class="dbtn quiet" data-dact="sk:view">Cancel</button></div></div></div>` : "";
    return deskScene(dBar(top + confirm + `<div class="dbody" style="padding-top:12px">
      <div class="dmuted">${k.when} · ${k.uses} uses</div>
      <div class="reply" style="display:grid;gap:10px;font-size:inherit;line-height:inherit"><div><b>Steps</b><ol style="margin:4px 0 0;padding-left:20px;display:grid;gap:2px">${k.body.map(b => `<li>${b}</li>`).join("")}</ol></div>
      <div><b>Preferences</b><ul style="margin-top:4px">${k.prefs.map(b => `<li>${b}</li>`).join("")}</ul></div></div>
      <div class="dmuted" style="border-top:1px solid var(--d-border);padding-top:10px"><span style="flex:1">Beep changed this last.</span><button class="dbtn" data-toast="Back to the earlier version">Undo<kbd>⌘Z</kbd></button></div>
    </div>` + dHints(st === "delete" ? [[DI.ret, "delete"], ["esc", "cancel"]] : [["space", "on or off"], [`⌘${DI.ret}`, "edit"], ["⌘Z", "undo"], ["esc", "back"]])));
  }
  const row = (x, cur) => {
    const meta = x.on ? `${x.when} · ${x.uses} uses${x.by === "you" ? " · edited by you" : ""}` : "Paused";
    if (cur && st === "rowdelete") return `<div class="drt" aria-selected="true"><span class="tt"><span style="font-size:14px">Delete <b>${x.name}</b>?</span></span><span class="acts"><button class="dbtn warn" data-dact="sk:list" data-toast="Skill deleted">Delete</button><button class="dbtn quiet" data-dact="sk:list">Cancel</button></span></div>`;
    const tail = cur && st === "actions"
      ? `<span class="acts"><button class="dbtn" data-dact="sk:view">Open<kbd>${DI.ret}</kbd></button>${x.i === 0 ? `<button class="dbtn" data-toast="Back to the earlier version">Undo<kbd>⌘Z</kbd></button>` : ""}<button class="dbtn warn" data-dact="sk:rowdelete">Delete<kbd>⌘⌫</kbd></button></span>`
      : `<button class="dswitch" role="switch" aria-checked="${x.on}" aria-label="Use ${x.name}" data-toast="${x.on ? "Paused" : "Resumed"}"></button>`;
    return `<div class="drt${x.on ? "" : " off"}" role="option" aria-selected="${cur}" data-dact="sk:view" style="cursor:pointer"><span class="ic">${sic}</span><span class="tt"><b>${x.name}</b><span>${x.desc}</span><small>${meta}</small></span>${tail}</div>`;
  };
  const own = SKILLS.map((x, i) => row({ ...x, i }, i === sel)).join("");
  const groups = GROUP_SKILLS.map(x => `<div class="dsec flat" style="padding:8px 10px 4px">${x.group}</div><div class="dlist" style="padding:0">${row({ ...x, i: 9 }, false)}</div>`).join("");
  return deskScene(dBar(head() + `<div class="dlist" role="listbox" aria-label="Skills" style="flex:1;min-height:0;padding-top:4px;overflow:auto;scrollbar-width:none">
    <div class="dsec flat" style="padding:8px 10px 4px">Yours</div><div class="dlist" style="padding:0">${own}</div>${groups}</div>`
    + dHints(st === "actions" ? [[DI.ret, "open"], ["⌘Z", "undo"], ["⌘⌫", "delete"], ["tab", "hide actions"], ["esc", "back"]] : [["↑↓", "move"], [DI.ret, "open"], ["space", "on or off"], ["tab", "actions"], ["esc", "back"]])));
};
