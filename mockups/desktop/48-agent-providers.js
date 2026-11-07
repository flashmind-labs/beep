
// Coding agents per machine, as T3's Providers: each harness with its
// version, sign-in and an on/off switch; the picked one's runtime settings.
(() => {
  const st = document.createElement("style");
  st.textContent = `.mac .pv { display: grid; grid-template-columns: 300px minmax(0, 1fr); gap: 0; margin: 14px 18px 18px; border: 1px solid var(--d-border); border-radius: 12px; overflow: hidden; min-height: 0 }
.mac .pv-list { border-right: 1px solid var(--d-border); display: grid; align-content: start; background: rgba(255,255,255,.015) }
.mac .pv-it { cursor: pointer; display: grid; grid-template-columns: 28px minmax(0, 1fr) 40px; gap: 10px; align-items: center; padding: 10px 14px; border-bottom: 1px solid var(--d-border) }
.mac .pv-it > span:nth-child(2), .mac .pv-sum > span:nth-child(2) { min-width: 0 }
.mac .pv-nm { display: flex; align-items: baseline; gap: 6px; min-width: 0; white-space: nowrap }
.mac .pv-nm b { overflow: hidden; text-overflow: ellipsis; flex: 0 1 auto; min-width: 0 }
.mac .pv-nm .ver { flex: 0 1000 auto; min-width: 0; overflow: hidden; text-overflow: ellipsis; margin-left: 0 !important; font-family: var(--mono); font-size: 11.5px; color: var(--d-muted) }
.mac .pv-sum + .pv-d > .pv-acts { margin-top: 10px }
.mac .pv-it .dswitch, .mac .pv-sum .dswitch { justify-self: end }
.mac .pv-it small, .mac .pv-sum small { white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
.mac .pv-bar { display: flex; align-items: center; gap: 8px; padding: 0 0 12px; margin-bottom: 14px; border-bottom: 1px solid var(--d-border); min-width: 0 }
.mac .pv-bar .lbl { font-size: 12px; color: var(--d-muted); flex: none }
.mac .pv-bar .sp { flex: 1 }
.mac .pv-bar .dbtn { flex: none; white-space: nowrap }
.mac .pv-seg { display: inline-flex; flex-wrap: nowrap; min-width: 0; padding: 2px; gap: 2px; border: 1px solid var(--d-border); border-radius: 8px; background: rgba(255,255,255,.02) }
.mac .pv-seg button { all: unset; cursor: pointer; height: 24px; padding: 0 10px; border-radius: 6px; font-size: 12.5px; color: var(--d-muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; max-width: 150px }
.mac .pv-seg button[aria-pressed="true"] { background: rgba(94,106,210,.28); color: var(--d-fg) }
.mac .pv-seg button:disabled { opacity: .45; cursor: default }
.mac .pv-sum { display: grid; grid-template-columns: 28px minmax(0, 1fr) auto 40px; gap: 12px; align-items: center; padding: 12px 14px; border: 1px solid var(--d-border); border-radius: 8px }
.mac .pv-sum b { font-size: 15px; font-weight: 600 }
.mac .pv-sum small { display: block; margin-top: 2px; font-size: 12px; color: var(--d-muted) }
.mac .pv-sum .on { font-size: 12px; color: var(--d-muted); white-space: nowrap }
.mac .pv-it[aria-current="true"] { background: rgba(94,106,210,.07) }
.mac .pv-it b { font-weight: 500; font-size: 13.5px }
.mac .pv-it .ver { font-family: var(--mono); font-size: 11.5px; color: var(--d-muted); margin-left: 6px }
.mac .pv-up { flex: none; font-size: 10.5px; line-height: 16px; padding: 0 6px; border-radius: 999px; color: #F2C94C; border: 1px solid rgba(242,201,76,.45); background: rgba(242,201,76,.08) }
.mac .pv-it small { display: block; margin-top: 3px; font-size: 12px; line-height: 17px; color: var(--d-muted) }
.mac .pv-it small.bad { color: var(--d-coral) }
.mac .pv-it.off b, .mac .pv-it.off .cc-ag { opacity: .45 }
.mac .pv-add { all: unset; cursor: pointer; padding: 12px 16px; font-size: 13px; color: var(--d-muted) }
.mac .pv-add:hover { color: var(--d-fg) }
.mac .pv-d { padding: 16px 20px; overflow-y: auto; scrollbar-width: thin; container-type: inline-size; min-width: 0 }
.mac .cs-sheet .pv-d { padding: 0; overflow: visible }
.mac .pv-d h3 { margin: 0; display: flex; align-items: center; gap: 10px; font-size: 17px; font-weight: 600 }
.mac .pv-d h3 .ver { margin-left: auto; font-family: var(--mono); font-size: 12px; color: var(--d-muted); font-weight: 400 }
.mac .pv-d h4 { margin: 20px 0 4px; font-size: 13.5px; font-weight: 600 }
.mac .pv-f { display: grid; grid-template-columns: 200px minmax(0, 1fr); gap: 20px; align-items: start; padding: 11px 0; border-bottom: 1px solid var(--d-border) }
.mac .pv-f > :last-child { min-width: 0; align-self: center }
@container (max-width: 480px) { .mac .pv-f { grid-template-columns: minmax(0, 1fr); gap: 8px } }
.mac .pv-f b { font-weight: 500; font-size: 13px }
.mac .pv-f small { display: block; margin-top: 2px; font-size: 12px; line-height: 17px; color: var(--d-muted) }
.mac .pv-in { height: 32px; padding: 0 11px; border-radius: 8px; border: 1px solid var(--d-border); background: #0f1011; display: flex; align-items: center; font-family: var(--mono); font-size: 12.5px; color: var(--d-fg); min-width: 0; overflow: hidden; white-space: nowrap }
.mac .pv-in.ph { color: var(--d-dim) }
.mac .pv-env { display: grid; grid-template-columns: minmax(0, 2fr) minmax(0, 3fr) 24px; gap: 8px; align-items: center }
.mac .pv-env + .pv-env { margin-top: 6px }
.mac .pv-diff { display: inline-block; margin-left: 6px; font-size: 10.5px; line-height: 15px; padding: 0 6px; border-radius: 999px; color: var(--d-muted); border: 1px solid var(--d-border); font-weight: 400; cursor: help }
.mac .pv-save { display: flex; flex-wrap: wrap; gap: 8px; align-items: center; margin-top: 12px; padding-top: 12px }
.mac .pv-save label { display: inline-flex; gap: 6px; align-items: center; font-size: 12px; color: var(--d-muted) }
.mac .pv-save .ok { font-size: 12px; color: var(--d-emerald) }
.mac .pv-env button { all: unset; cursor: pointer; color: var(--d-muted); text-align: center }
.mac .fb-err { margin: 14px 18px 0; padding: 10px 12px; border: 1px solid color-mix(in srgb, var(--d-coral) 45%, transparent); border-radius: 8px; background: color-mix(in srgb, var(--d-coral) 8%, transparent); font-size: 12.5px; min-width: 0 }
.mac .fb-line { display: flex; gap: 10px; align-items: flex-start; color: var(--d-coral); line-height: 18px }
.mac .fb-line span { flex: 1; min-width: 0; overflow-wrap: anywhere }
.mac .fb-x { all: unset; cursor: pointer; color: var(--d-muted); padding: 0 4px }
.mac .fb-more summary { margin-top: 6px; cursor: pointer; color: var(--d-muted); font-size: 12px }
.mac .fb-more pre { margin: 6px 0 0; max-height: 180px; overflow: auto; padding: 8px 10px; border-radius: 8px; background: #0f1011; border: 1px solid var(--d-border); font-family: var(--mono); font-size: 11.5px; line-height: 16px; white-space: pre-wrap; overflow-wrap: anywhere; color: var(--d-fg) }
.mac .pv-stat { display: flex; gap: 8px; align-items: center; font-size: 12.5px; color: var(--d-muted); padding: 6px 0 }
.mac .fl-seg, .mac .fl-chips { display: flex; flex-wrap: wrap; gap: 4px }
.mac .fl-chips .dchip { font-family: var(--mono); font-size: 11.5px }`;
  document.head.append(st);

  const AGK = { claude: ["CC", "Claude Code", "claude", "CLAUDE_CONFIG_DIR", "~/.claude"], codex: ["CX", "Codex", "codex", "CODEX_HOME", "~/.codex"], gemini: ["GE", "Gemini CLI", "gemini", "GEMINI_HOME", "~/.gemini"], pi: ["PI", "pi", "pi", "PI_HOME", "~/.pi"], flsh: ["FL", "flsh", "flsh", "FLASHMIND_HOME", "~/.flashmind"] };
  const av = a => `<span class="cc-ag ${a}">${AGK[a][0]}</span>`;
  const sw = on => `<button class="dswitch" role="switch" aria-checked="${on}" data-toast="${on ? "Turned off on build-box: Auto stops picking it here" : "Turned on"}"></button>`;
  const MACHINES = [["build-box", true], ["mac-box", true], ["win-box", false]];
  const pick = (key, ok, def) => ok.includes(S.d[key]) ? S.d[key] : def;
  // A failed update: one line with the cleaned error, the whole output behind Details, and a dismiss.
  const failBanner = `<div class="fb-err" role="alert"><div class="fb-line"><span>Could not update OpenCode on mac-box: opencode upgrade failed: Error: Unexpected error, check log file at ~/.local/share/opencode/log/2026-11-03.log for more details SyntaxError: JSON Parse error: Unexpected EOF (tried brew upgrade opencode, then opencode upgrade)</span><button class="fb-x" aria-label="Dismiss" data-toast="Dismissed">&#215;</button></div><details class="fb-more"><summary>Details</summary><pre>$ /opt/homebrew/bin/brew upgrade opencode
Error: Refusing to load formula example/tap/opencode from untrusted tap example/tap.

$ /opt/homebrew/bin/opencode upgrade
Error: Unexpected error, check log file at ~/.local/share/opencode/log/2026-11-03.log for more details
SyntaxError: JSON Parse error: Unexpected EOF</pre></details></div>`;
  D.dProviders = () => SETV.providers(S.d.pv || "codex", failBanner);
  VARIANTS.dProviders = [["Codex", "codex"], ["Claude Code", "claude"], ["Claude not signed in", "signin"], ["Gemini not installed", "missing"], ["Updating", "update"], ["Update failed", "fail"]]
    .map(([l, k]) => [l, () => { S.d.pv = k; render(); }, () => pick("pv", ["codex", "claude", "signin", "missing", "update", "fail"], "codex") === k]);
  DESK_SCREENS.find(g => g.group === "Command center").items.push(["dProviders", "Coding agents (providers)", "CommandCenterWindow, ProvidersPage"]);
})();
