// ----- Diff viewer -----
// A coding session's Changes tab (beside Files) and a branch's or task's diff (View diff on
// Branches, the review and Tasks). The node runs git in the checkout: committed changes since
// the base plus uncommitted and untracked ones. Each file folds; Expand all / Collapse all and a
// filter sit on top; a wide view lists the files on the left. Files over 400 changed lines and
// deleted files start folded, lockfiles and files past 2,000 lines come without their lines.
{
  const st = document.createElement("style");
  st.textContent = ``;
  document.head.append(st);
}

// Phone: Settings, Instructions, and a proposal card.
const IN_P_DIFF = (lines) => `<div style="display:grid;border-radius:var(--radius);overflow:hidden;border:1px solid var(--m-soft);font-family:var(--mono);font-size:11.5px;line-height:18px">${lines.map(([k, t]) =>
  `<span style="padding:2px 10px;white-space:pre-wrap;${k === "add" ? "background:#DDF3E8;color:#1E6B4F" : k === "del" ? "background:#FBE3DF;color:#A3352A" : "color:var(--m-muted)"}">${k === "add" ? "+ " : k === "del" ? "- " : "  "}${t}</span>`).join("")}</div>`;
R.instructions = () => back("Instructions", `<button class="text-btn" data-toast="Versions: 7 saved">History</button>`) + `<div class="body" style="gap:10px">
    <div class="muted" style="font-size:13px;line-height:18px;padding:0 4px">Your rules for every agent Beep runs, on every machine.</div>
    <button class="banner" style="background:var(--m-t2);border:0;text-align:left;width:100%;font:inherit;color:inherit" data-go="instrProposal"><span style="flex:1"><b style="font-weight:600">3 proposals from Beep</b><br><span class="muted" style="font-size:13px">Nothing changes until you accept</span></span><span style="display:grid;transform:rotate(-90deg)">${I.chev}</span></button>
    <div class="muted" style="font-size:12px;padding:0 4px">Version 7 · you · 2 days ago</div>
    <textarea class="field" rows="13" style="font-family:var(--mono);font-size:12px;line-height:18px">${IN_RULES.replace(/\n  (?=\S)/g, " ")}</textarea>
    <div class="muted" style="font-size:12px;padding:0 4px">0.6 of 16 KB · after Beep's base, before the project's</div>
    <div style="display:flex;gap:10px">
      <button class="pill chunky" style="flex:1;font-weight:600" data-toast="Beep reads it and proposes edits">Improve</button>
      <button class="pill chunky" style="flex:1;font-weight:600" data-toast="Import from machines reads the agent files on your machines">Import</button></div>
    <div class="end"><button class="go" style="box-shadow:0 3px 0 var(--m-ledge)" data-toast="Saved as version 8">Save</button></div></div>`;
R.instrProposal = () => `<div class="bar" style="padding-left:6px"><button class="ib" data-go="instructions" aria-label="Back">${I.back}</button><div class="title" style="flex:1">Proposal</div><span class="muted" style="font-size:13px;padding-right:14px">1 of 3</span></div>
  <div class="body" style="gap:12px">
    <div class="card" style="display:grid;gap:10px">
      <div style="display:flex;align-items:center;gap:8px"><b style="font-weight:600;font-size:15px;flex:1">Add to your instructions?</b><span class="tag" style="background:var(--m-t1)">For all agents</span></div>
      ${IN_P_DIFF([["ctx", "- Ask before deploying."], ["add", "- Never post on GitHub yourself."], ["add", "- Draft the reply in chat."]])}
      <div style="font-size:14px;line-height:20px">You told agents this in three chats this month, after they tried to post a PR comment.</div>
      <div class="chips" style="padding:0">${["Said in 3 chats", "1 memory"].map(t => `<button class="fchip" aria-pressed="false" data-toast="Opens where it came from">${t}</button>`).join("")}</div>
      <div class="muted" style="font-size:12px">Repeated correction · found last night</div>
    </div>
    <div style="flex:1"></div>
    <div style="display:flex;gap:10px;align-items:center">
      <button class="text-btn" style="color:var(--m-error)" data-go="instructions" data-toast="Dismissed. Beep won't propose this again">Dismiss</button>
      <div style="flex:1"></div>
      <button class="pill chunky" style="font-weight:600;padding:0 18px" data-toast="Edit the line before accepting">Edit</button>
      <button class="go" style="box-shadow:0 3px 0 var(--m-ledge)" data-go="instructions" data-toast="Accepted. Saved as version 8">Accept</button></div>
  </div>`;
SCREENS.find(g => g.group === "Settings").items.push(["instructions", "Instructions", "InstructionsScreen"], ["instrProposal", "Instructions proposal", "InstructionsScreen, ProposalCard"]);
{
  const base = R.settings;
  R.settings = () => base().replace("<span>What routines may do without asking</span></div></button>",
    `<span>What routines may do without asking</span></div></button>
      <button class="setrow" data-go="instructions"><span class="logo" style="background:${TINTS[2]}">${I.edit}</span><div class="grow"><b>Instructions</b><span>Your rules for every agent · 3 proposals</span></div></button>`);
}
