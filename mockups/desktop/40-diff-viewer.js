// ----- Diff viewer -----
// A coding session's Changes tab (beside Files) and a branch's or task's diff (View diff on
// Branches, the review and Tasks). The node runs git in the checkout: committed changes since
// the base plus uncommitted and untracked ones. Each file folds; Expand all / Collapse all and a
// filter sit on top; a wide view lists the files on the left. Files over 400 changed lines and
// deleted files start folded, lockfiles and files past 2,000 lines come without their lines.
{
  const st = document.createElement("style");
  st.textContent = `
.mac .dv { --fg: var(--d-fg); --muted: var(--d-muted); --dim: var(--d-dim); --border: var(--d-border); --elev: var(--d-elev); --surface: var(--d-surface); --emerald: var(--d-emerald, #5E6AD2); --err: var(--d-err, #EB5757);
  --dv-add-bg: rgba(94,106,210,.15); --dv-add-gut: rgba(94,106,210,.24); --dv-add-fg: #8A9CF0; --dv-del-bg: rgba(242,153,74,.13); --dv-del-gut: rgba(242,153,74,.22); --dv-del-fg: #F4A196;
  --dv-hunk-bg: rgba(111,168,220,.09); --dv-hunk-fg: #8A9CF0; --dv-file-bg: rgba(21,22,23,.82); --dv-head-bg: #1A1B1D; --dv-gut-bg: #151617 }
.mac .dv.light { --fg: #1B1D2E; --muted: #5D6168; --dim: #8A8F98; --border: #D3E0D7; --elev: #F4F8F5; --surface: #FFFFFF;
  --dv-add-bg: #E5F5EC; --dv-add-gut: #CDEBDA; --dv-add-fg: #5E6AD2; --dv-del-bg: #FCEAE7; --dv-del-gut: #F6D3CD; --dv-del-fg: #B23A2B;
  --dv-hunk-bg: #EAF2FB; --dv-hunk-fg: #2F6CA8; --dv-file-bg: #FFFFFF; --dv-head-bg: #F4F8F5; --dv-gut-bg: #F7FAF8; background: #EEF3EF; color: var(--fg) }
.mac .dv { display: flex; flex-direction: column; min-height: 0; min-width: 0; flex: 1; font-size: 12.5px; color: var(--fg) }
.mac .dv-head { flex: none; display: flex; flex-wrap: wrap; align-items: center; gap: 6px 8px; padding: 8px 10px; border-bottom: 1px solid var(--border) }
.mac .dv-sum { display: flex; align-items: baseline; gap: 6px; min-width: 0; white-space: nowrap; font-variant-numeric: tabular-nums } .mac .dv-sum b { font-weight: 500 }
.mac .dv-sum .base { color: var(--muted); font-size: 11.5px; overflow: hidden; text-overflow: ellipsis }
.mac .dv-plus { color: var(--dv-add-fg); font-style: normal } .mac .dv-minus { color: var(--dv-del-fg); font-style: normal }
.mac .dv-sp { flex: 1 }
.mac .dv-find { flex: 0 1 200px; min-width: 110px; height: 26px; padding: 0 8px; border-radius: 7px; border: 1px solid var(--border); background: var(--elev); color: var(--fg); font: 12px var(--body); outline: 0 }
.mac .dv-btn { height: 26px; padding: 0 9px; border-radius: 7px; border: 1px solid var(--border); background: transparent; color: var(--fg); font: 12px var(--body); white-space: nowrap }
.mac .dv-btn.icon { width: 26px; padding: 0; display: inline-grid; place-items: center; color: var(--muted) }
.mac .dv-body { flex: 1; min-height: 0; display: grid; grid-template-columns: minmax(0, 1fr) }
.mac .dv.wide .dv-body { grid-template-columns: 250px minmax(0, 1fr) }
.mac .dv-tree { display: none; min-height: 0; overflow: auto; padding: 6px; border-right: 1px solid var(--border) } .mac .dv.wide .dv-tree { display: block }
.mac .dv-tr { display: grid; grid-template-columns: 14px minmax(0, 1fr) auto; gap: 8px; align-items: center; width: 100%; padding: 3px 6px; border: 0; border-radius: 5px; background: transparent; color: var(--fg); text-align: left; font: 12px var(--mono) }
.mac .dv-tr.on { background: rgba(94,106,210,.2) } .mac .dv-tr .nm { display: flex; min-width: 0; white-space: nowrap } .mac .dv-tr .nm i { flex: 0 1 auto; min-width: 0; overflow: hidden; text-overflow: ellipsis; color: var(--muted); font-style: normal } .mac .dv-tr .nm b { flex: 0 0 auto; max-width: 100%; overflow: hidden; text-overflow: ellipsis; font-weight: 400 } .mac .dv-tr .ct { font-size: 11px; display: flex; gap: 6px }
.mac .dv-st { font: 600 10.5px var(--mono); text-align: center; color: var(--muted) } .mac .dv-st.A { color: var(--dv-add-fg) } .mac .dv-st.D { color: var(--dv-del-fg) } .mac .dv-st.M { color: #E5B567 } .mac .dv-st.R { color: var(--dv-hunk-fg) }
.mac .dv-files { min-height: 0; min-width: 0; overflow: auto; padding: 0 10px 10px; display: flex; flex-direction: column; gap: 10px }
.mac .dv-file { flex: none; min-width: 0; border: 1px solid var(--border); border-radius: 9px; background: var(--dv-file-bg) } .mac .dv-file:first-child { margin-top: 10px }
.mac .dv-fh { position: sticky; top: 0; z-index: 2; display: flex; align-items: center; gap: 8px; min-height: 34px; padding: 4px 6px 4px 8px; border-radius: 9px 9px 0 0; background: var(--dv-head-bg); border-bottom: 1px solid var(--border); cursor: pointer }
.mac .dv-file.closed .dv-fh { border-radius: 9px; border-bottom-color: transparent } .mac .dv-file.cur .dv-fh { box-shadow: inset 2px 0 0 var(--emerald) }
.mac .dv-chev { flex: none; display: grid; color: var(--muted); transition: transform .18s } .mac .dv-file:not(.closed) .dv-chev { transform: rotate(90deg) }
.mac .dv-path { flex: 1; min-width: 0; display: flex; align-items: center; gap: 6px; font: 12px var(--mono); white-space: nowrap; overflow: hidden } .mac .dv-path > span { overflow: hidden; text-overflow: ellipsis }
.mac .dv-path .old { color: var(--muted); flex-shrink: 3 } .mac .dv-path .to { flex: none; color: var(--dim); display: grid }
.mac .dv-chip { flex: none; font-size: 10.5px; line-height: 16px; padding: 0 6px; border-radius: 999px; border: 1px solid var(--border); color: var(--muted) }
.mac .dv-chip.added { color: var(--dv-add-fg) } .mac .dv-chip.deleted { color: var(--dv-del-fg) } .mac .dv-chip.modified { color: #E5B567 } .mac .dv-chip.renamed { color: var(--dv-hunk-fg) }
.mac .dv-fct { flex: none; display: flex; gap: 6px; font-size: 11.5px } .mac .dv-facts { flex: none; display: flex; gap: 2px } .mac .dv-facts .dv-btn.icon { width: 24px; height: 24px; border-color: transparent }
.mac .dv-fold { display: grid; grid-template-rows: 1fr; transition: grid-template-rows .2s ease } .mac .dv-file.closed .dv-fold { grid-template-rows: 0fr } .mac .dv-fold > div { min-height: 0; overflow: hidden }
@media (prefers-reduced-motion: reduce) { .mac .dv-fold, .mac .dv-chev { transition: none } }
.mac .dv-scroll { overflow-x: auto; overflow-y: hidden; border-radius: 0 0 9px 9px } .mac .dv-lines { width: max-content; min-width: 100%; font: 12px/19px var(--mono) }
.mac .dv-l { display: flex; min-width: 100% } .mac .dv-g { position: sticky; left: 0; z-index: 1; flex: none; display: flex; background: var(--dv-gut-bg); color: var(--dim); border-right: 1px solid var(--border) }
.mac .dv-g span { width: calc(var(--nw, 3) * 1ch + 14px); padding: 0 7px; text-align: right } .mac .dv-c { flex: 1; padding: 0 12px 0 6px; white-space: pre }
.mac .dv-c::before { display: inline-block; width: 1.2ch; color: var(--dim); content: " " }
.mac .dv-lines.split { width: 100% } .mac .dv-pair { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr) } .mac .dv-pair .dv-l { min-width: 0 } .mac .dv-pair .dv-l + .dv-l { border-left: 1px solid var(--border) }
.mac .dv-pair .dv-c { min-width: 0; white-space: pre-wrap; overflow-wrap: anywhere } .mac .dv-l.none { background: var(--dv-gut-bg) } .mac .dv-l.none .dv-c::before { content: none }
.mac .dv-splitbtn[aria-pressed="true"] { background: var(--elev) } @container (max-width: 640px) { .mac .dv-splitbtn { display: none } }
.mac .dv-l.add { background: var(--dv-add-bg) } .mac .dv-l.add .dv-g { background: var(--dv-add-gut); color: var(--dv-add-fg) } .mac .dv-l.add .dv-c::before { content: "+"; color: var(--dv-add-fg) }
.mac .dv-l.del { background: var(--dv-del-bg) } .mac .dv-l.del .dv-g { background: var(--dv-del-gut); color: var(--dv-del-fg) } .mac .dv-l.del .dv-c::before { content: "-"; color: var(--dv-del-fg) }
.mac .dv-l.hunk { background: var(--dv-hunk-bg); color: var(--dv-hunk-fg) } .mac .dv-l.hunk .dv-g { background: var(--dv-hunk-bg) } .mac .dv-l.hunk .dv-c { font-size: 11.5px } .mac .dv-l.hunk .dv-c::before { content: none }
.mac .dv-l.gap { color: var(--muted); font: 11.5px/22px var(--body) } .mac .dv-l.gap .dv-c::before { content: none }
.mac .dv-eol { margin-left: 8px; padding: 0 5px; border-radius: 4px; border: 1px solid var(--border); color: var(--muted); font: 10.5px/15px var(--body) }
.mac .dv-note { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; padding: 10px 12px; color: var(--muted); font-size: 12px } .mac .dv-note .dv-btn { height: 24px }
.mac .dv-empty { margin: auto; padding: 24px; display: grid; justify-items: center; gap: 6px; text-align: center; color: var(--muted); font-size: 13px } .mac .dv-empty b { color: var(--fg); font-weight: 500 }
.mac .dv-keys { flex: none; display: flex; gap: 12px; padding: 5px 12px; border-top: 1px solid var(--border); color: var(--dim); font-size: 11px }
.mac .dv-keys kbd { margin-right: 4px; padding: 0 4px; border: 1px solid var(--border); border-radius: 4px; font: 10.5px var(--mono); color: var(--muted) }
.mac .dv-wrap { flex: 1; min-height: 0; display: flex; flex-direction: column }`;
  document.head.append(st);

  const CHEV = '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><path d="M9 6l6 6-6 6"/></svg>';
  const TO = '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12h14M13 6l6 6-6 6"/></svg>';
  const COPY = '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><rect x="9" y="9" width="11" height="11" rx="2"/><path d="M5 15V5a2 2 0 0 1 2-2h8"/></svg>';
  const OPEN = '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M14 3H6v18h12V7z"/><path d="M14 3v4h4"/></svg>';
  const REFRESH = '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M20 11a8 8 0 1 0-2.3 5.7M20 4v7h-7"/></svg>';
  const esc = t => t.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  // [kind, text]: " " context, "+" added, "-" removed; "@" starts a hunk: [old, new, section].
  const LOGIN = [["@", 12, 12, "pub fn login(user: &str, pass: &str) -> Result<Session, LoginError> {"],
    [" ", "    let user = find(user)?;"], [" ", "    if !check(&user, pass) {"],
    ["-", '        return Err(LoginError::new("Wrong password for this email"));'],
    ["+", '        return Err(LoginError::new("Wrong email or password"));'], [" ", "    }"], [" ", "    Ok(Session::new(user))"],
    ["@", 48, 48, "fn find(email: &str) -> Result<User, LoginError> {"],
    [" ", "    let found = USERS.lock().get(email).cloned();"],
    ["-", "    found.ok_or(LoginError::UnknownEmail)"],
    ["+", "    // Same message as a wrong password, so the screen never says which accounts exist; the audit log keeps the real reason for support."],
    ["+", "    found.ok_or(LoginError::new(\"Wrong email or password\"))"], [" ", "}"]];
  const RENAME = [["@", 1, 1, ""], ["-", "pub struct Session { id: u64 }"], ["+", "pub struct Session { id: u64, expires: Instant }"], [" ", ""], [" ", "impl Session {"]];
  const TEST = [["@", 0, 1, ""], ["+", "use widget_core::login;"], ["+", ""], ["+", "#[test]"], ["+", "fn wrong_email_reads_like_wrong_password() {"],
    ["+", '    let a = login("nobody@example.com", "x").unwrap_err();'], ["+", '    let b = login("user@example.com", "x").unwrap_err();'], ["+", "    assert_eq!(a.to_string(), b.to_string());"], ["+", "}", 1]];
  const FILES = [
    { path: "crates/core/src/login.rs", st: "modified", a: 3, d: 2, lines: LOGIN },
    { path: "crates/core/src/auth/sessions.rs", old: "crates/core/src/auth/session.rs", st: "renamed", a: 1, d: 1, lines: RENAME },
    { path: "crates/core/tests/login_flow.rs", st: "added", a: 8, d: 0, lines: TEST },
    { path: "apps/web/public/logo.png", st: "binary", a: 0, d: 0, note: "Binary file, not shown." },
    { path: "crates/core/src/old_api.rs", st: "deleted", a: 0, d: 64, lines: [["@", 1, 0, ""], ["-", "//! The v1 login API, kept for the 2.x apps."], ["-", "pub fn login_v1() {}"]], fold: true },
    { path: "crates/core/src/generated/schema.rs", st: "modified", a: 512, d: 98, lines: [["@", 1, 1, ""], [" ", "// @generated"], ["-", "pub const VERSION: u32 = 41;"], ["+", "pub const VERSION: u32 = 42;"]], fold: true },
    { path: "apps/web/src/strings.json", st: "modified", a: 1840, d: 1203, note: "Too large to show: 3,043 changed lines.", load: "Load anyway", fold: true },
    { path: "Cargo.lock", st: "modified", a: 24, d: 18, note: "Lockfile, 42 changed lines.", load: "Show", fold: true },
  ];
  const LETTER = { added: "A", modified: "M", deleted: "D", renamed: "R", binary: "B" };

  let dvSplit = false;
  // Side by side: a run of removed lines sits across from the added lines that follow it.
  const splitRows = f => {
    let o = 0, n = 0, rows = "", dels = [], adds = [];
    const half = (cls, num, text) => `<div class="dv-l${cls}"><span class="dv-g"><span>${num}</span></span><span class="dv-c">${text}</span></div>`;
    const flush = () => {
      for (let i = 0; i < Math.max(dels.length, adds.length); i++) {
        rows += `<div class="dv-pair">${dels[i] ? half(" del", dels[i][0], esc(dels[i][1])) : half(" none", "", "")}${adds[i] ? half(" add", adds[i][0], esc(adds[i][1])) : half(" none", "", "")}</div>`;
      }
      dels = []; adds = [];
    };
    for (const l of f.lines) {
      if (l[0] === "@") {
        flush();
        o = Math.max(l[1] - 1, 0); n = Math.max(l[2] - 1, 0);
        rows += `<div class="dv-l hunk"><span class="dv-g"><span>...</span></span><span class="dv-c">@@ -${l[1]} +${l[2]} @@ ${esc(l[3])}</span></div>`;
      } else if (l[0] === "-") dels.push([++o, l[1]]);
      else if (l[0] === "+") adds.push([++n, l[1]]);
      else { flush(); rows += `<div class="dv-pair">${half("", ++o, esc(l[1]))}${half("", ++n, esc(l[1]))}</div>`; }
    }
    flush();
    return `<div class="dv-lines split" style="--nw:2">${rows}</div>`;
  };
  const lineRows = f => {
    if (dvSplit) return splitRows(f);
    let o = 0, n = 0, rows = "", prevEnd = null;
    for (const l of f.lines) {
      if (l[0] === "@") {
        o = Math.max(l[1] - 1, 0); n = Math.max(l[2] - 1, 0);
        if (prevEnd != null && o - prevEnd > 0) rows += `<div class="dv-l gap"><span class="dv-g"><span></span><span></span></span><span class="dv-c">${o - prevEnd} unchanged lines</span></div>`;
        rows += `<div class="dv-l hunk"><span class="dv-g"><span>...</span><span>...</span></span><span class="dv-c">@@ -${l[1]} +${l[2]} @@ ${esc(l[3])}</span></div>`;
        continue;
      }
      const cls = l[0] === "+" ? " add" : l[0] === "-" ? " del" : "";
      const on = l[0] === "+" ? "" : ++o, nn = l[0] === "-" ? "" : ++n;
      rows += `<div class="dv-l${cls}"><span class="dv-g"><span>${on}</span><span>${nn}</span></span><span class="dv-c">${esc(l[1])}${l[2] ? '<span class="dv-eol" title="No newline at end of file">no newline at end</span>' : ""}</span></div>`;
      prevEnd = o;
    }
    return `<div class="dv-scroll"><div class="dv-lines" style="--nw:2">${rows}</div></div>`;
  };
  const fileBox = (f, closed, cur, pane) => {
    const path = f.old ? `<span class="old" title="${f.old}">${f.old}</span><span class="to" title="renamed to">${TO}</span><span title="${f.path}">${f.path}</span>` : `<span title="${f.path}">${f.path}</span>`;
    const body = f.lines ? lineRows(f) : `<div class="dv-note">${f.note}${f.load ? `<button class="dv-btn" data-toast="${f.load === "Show" ? "Shows the lockfile's 42 lines" : "Loads all 3,043 lines of this file"}">${f.load}</button>` : ""}</div>`;
    const open = f.st === "deleted" ? "" : `<button class="dv-btn icon" title="${pane ? "Open in a tab" : "Show in Files"}" data-toast="${pane ? "Opens " + f.path + " in a tab" : "Shows it in Files"}">${OPEN}</button>`;
    return `<section class="dv-file${closed ? " closed" : ""}${cur ? " cur" : ""}"><div class="dv-fh" data-toast="Folds or unfolds the file (x)" title="Click or press x to fold"><span class="dv-chev">${CHEV}</span><span class="dv-path">${path}</span><span class="dv-chip ${f.st}">${f.st}</span>
      <span class="dv-fct">${f.a ? `<i class="dv-plus">+${f.a}</i>` : ""}${f.d ? `<i class="dv-minus">-${f.d}</i>` : ""}</span><span class="dv-facts"><button class="dv-btn icon" title="Copy path" data-toast="Copied ${f.path}">${COPY}</button>${open}</span></div>
      <div class="dv-fold"><div>${body}</div></div></section>`;
  };
  // v: mixed (the default folds), expanded, collapsed, filter, pane, empty, light.
  const dvBody = (v, wide) => {
    dvSplit = v === "split" || (!wide && S.d.dv === "split");
    if (v === "empty") return `<div class="dv"><div class="dv-head"><span class="dv-sum"><b>No changes</b><span class="base">against <code>origin/main</code> at <code>3f2c1a0</code>, uncommitted included</span></span><span class="dv-sp"></span><input class="dv-find" placeholder="Filter files" aria-label="Filter files" disabled><button class="dv-btn" disabled title="Unfold every file">Expand all</button><button class="dv-btn" disabled title="Fold every file">Collapse all</button><button class="dv-btn icon" title="Read the changes again" data-toast="Read the changes again">${REFRESH}</button></div>
      <div class="dv-body"><div class="dv-files" style="position:relative"><div class="dv-empty"><b>Nothing changed</b><span>The folder matches origin/main, with nothing uncommitted.</span></div></div></div>
      <div class="dv-keys"><span><kbd>j</kbd><kbd>k</kbd>next, previous file</span><span><kbd>x</kbd>fold the file</span><span><kbd>/</kbd>filter</span></div></div>`;
    const q = v === "filter" ? "login" : "";
    const shown = FILES.filter(f => !q || f.path.includes(q));
    const fold = f => v === "collapsed" ? true : v === "expanded" ? false : !!f.fold;
    const add = FILES.reduce((s, f) => s + f.a, 0), del = FILES.reduce((s, f) => s + f.d, 0);
    const tree = shown.map((f, i) => {
      const cut = f.path.lastIndexOf("/");
      return `<button class="dv-tr${i === 0 ? " on" : ""}" title="${f.path}" data-toast="Jumps to ${f.path}"><span class="dv-st ${LETTER[f.st]}">${LETTER[f.st]}</span><span class="nm"><i>${f.path.slice(0, cut + 1)}</i><b>${f.path.slice(cut + 1)}</b></span><span class="ct">${f.a ? `<i class="dv-plus">+${f.a}</i>` : ""}${f.d ? `<i class="dv-minus">-${f.d}</i>` : ""}</span></button>`;
    }).join("");
    const files = shown.map((f, i) => fileBox(f, fold(f), i === 0, v === "pane")).join("");
    const more = q ? `<div class="dv-note" style="padding:2px">${shown.length} of ${FILES.length} files match</div>` : "";
    return `<div class="dv${wide ? " wide" : ""}${v === "light" ? " light" : ""}"><div class="dv-head"><span class="dv-sum"><b>${FILES.length} files</b><i class="dv-plus">+${add.toLocaleString("en-US")}</i><i class="dv-minus">-${del.toLocaleString("en-US")}</i><span class="base" title="Against origin/main at 3f2c1a0, uncommitted included">vs <code>origin/main</code></span></span>
      <input class="dv-find" placeholder="Filter" aria-label="Filter files" value="${q}"><span class="dv-tools">${v === "pane" ? `<button class="dv-btn icon" aria-pressed="false" title="Uncommitted only: hide the branch's commits">${DV_IC.dirty}</button>` : ""}<button class="dv-btn icon dv-splitbtn" aria-pressed="${dvSplit}" title="${dvSplit ? "Unified" : "Side by side"}" data-dact="dset:dv:${dvSplit ? "mixed" : "split"}">${DV_IC.split}</button><button class="dv-btn icon" title="${v === "collapsed" ? "Expand all" : "Collapse all"}" data-dact="dset:dv:${v === "collapsed" ? "expanded" : "collapsed"}">${v === "collapsed" ? DV_IC.unfold : DV_IC.fold}</button><button class="dv-btn icon" title="Read again" data-toast="Read the changes again">${REFRESH}</button></span><span class="dv-sp"></span><i class="dv-acts"></i></div>
      <div class="dv-body"><nav class="dv-tree" aria-label="Changed files">${tree}</nav><div class="dv-files">${more}${files}</div></div>
      <div class="dv-keys"><span><kbd>j</kbd><kbd>k</kbd>next, previous file</span><span><kbd>x</kbd>fold the file</span><span><kbd>/</kbd>filter</span></div></div>`;
  };
const DV_IC = {
  split: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M12 4v16"/></svg>',
  fold: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M7 20l5-5 5 5M7 4l5 5 5-5"/></svg>',
  unfold: '<span class="dv-pm">+</span>',
  dirty: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="12" cy="12" r="3"/><path d="M12 3v6M12 15v6"/></svg>',
};
  D.dWinDiff = forced => {
    const v = forced || pick("dv", ["mixed", "split", "expanded", "collapsed", "filter", "pane", "empty", "light"], "mixed");
    if (v === "pane") {
      const keep = S.d.ags;
      S.d.ags = "idle";
      try { return D.dAgentSession(); } finally { S.d.ags = keep; }
    }
    // A branch against its default branch: no working tree, no Show in Files.
    const body = dvBody(v, true).replace(/<button class="dv-btn icon" title="Show in Files"[\s\S]*?<\/button>/g, "")
      .replace(/origin\/main(<\/code> at <code>[0-9a-f]+<\/code>), (?:uncommitted included|with uncommitted changes)/, "main$1")
      .replace("The folder matches origin/main, with nothing uncommitted.", "The branch has no commits past main.");
    const sheet = `<div class="dv-sheet" role="dialog" aria-modal="true" aria-label="Changes on login-copy"><div class="box"><div class="top"><h3>login-copy</h3><small>acme/widget, against its default branch</small><span style="flex:1"></span><button class="dv-btn" title="Close (esc)" data-dact="dset:psel:branches" data-dgo="dWinProject">Close</button></div>${body}</div></div>`;
    const keep = S.d.psel;
    S.d.psel = "branches";
    try { return D.dWinProject().replace(/<\/div>\s*<\/div>\s*$/, "</div>" + sheet + "</div>"); } finally { S.d.psel = keep; }
  };
  VARIANTS.dWinDiff = [["Default folds (big and deleted folded)", "mixed"], ["Side by side", "split"], ["All expanded", "expanded"], ["All collapsed", "collapsed"], ["Filtered", "filter"], ["Session Changes tab", "pane"], ["No changes", "empty"], ["Light theme tokens", "light"]]
    .map(([l, k]) => [l, () => { S.d.dv = k; render(); }, () => (S.d.dv || "mixed") === k]);
  window.__dvBody = dvBody;
  const pick = (key, ok, def) => ok.includes(S.d[key]) ? S.d[key] : def;
}
