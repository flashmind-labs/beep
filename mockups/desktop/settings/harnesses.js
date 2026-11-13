const SETTINGS_HARNESSES = (() => {
  const { tile, swi, dsetGo, box } = SETTINGS_ACCOUNTS;
  const { note } = SETTINGS_CREDENTIALS;
  const HV = {
    hermes: { name: "Hermes", ver: "0.20.0", status: "Installed", list: "Installed", unknown: true, home: "HERMES_HOME", hdef: "~/.hermes", prog: "hermes" },
    claude: { name: "Claude Code", ver: "2.1.312", latest: "2.1.320", status: "Signed in as you@example.com · Max plan", list: "Signed in · Max plan", home: "CLAUDE_CONFIG_DIR", hdef: "~/.claude", prog: "claude", diffs: { program: ["mac-box", "win-box"], args: ["win-box"] } },
    codex: { name: "Codex", ver: "0.166.0", status: "Signed in as you@example.com · Pro plan", list: "Signed in · Pro plan", home: "CODEX_HOME", hdef: "~/.codex", prog: "codex" },
    gemini: { name: "Gemini CLI", missing: true, install: "npm i -g @google/gemini-cli" },
    pi: { name: "pi", ver: "0.99.2", status: "Installed", list: "Installed", unknown: true, home: "PI_HOME", hdef: "~/.pi", prog: "pi" },
    opencode: { name: "OpenCode", ver: "1.19.4", status: "Not signed in on this machine", list: "Not signed in on this machine", bad: true, out: true, home: "OPENCODE_HOME", hdef: "~/.opencode", prog: "opencode" },
    flsh: { name: "flsh", ver: "0.41.0", status: "Installed", list: "Installed", unknown: true, home: "FLASHMIND_HOME", hdef: "~/.flashmind", prog: "flsh" },
  };
  const hvSwitch = (a, on) => swi(on, `${HV[a].name} on build-box`, `data-toast="${on ? "Turned off on build-box" : "Turned on on build-box"}"`);
  const nameLine = (name, ver, latest) => `<span class="hv-nm"><b>${name}</b>${ver ? `<span class="ver">v${ver}</span>` : ""}${latest ? `<span class="hv-up" title="v${latest} is out">update</span>` : ""}</span>`;
  const statusSmall = (t, bad) => `<small${bad ? ' class="bad"' : ""} title="${t}">${t}</small>`;
  const hvPicker = () => `<label class="hv-machine-menu"><select aria-label="Machine runtime settings"><option>build-box</option><option>mac-box</option><option disabled>win-box (offline)</option></select></label>`;
  const hvIn = (val, ph, label) => `<input class="hv-in" value="${val}" placeholder="${ph}" spellcheck="false" autocomplete="off" aria-label="${label}">`;
  const hvField = (label, help, ctl, names = []) => `<div class="hv-f"><span><b>${label}</b>${names.length ? `<span class="hv-diff" title="Not the same on ${names.join(", ")}">differs on ${names.length} machine${names.length === 1 ? "" : "s"}</span>` : ""}<small>${help}</small></span>${ctl}</div>`;
  const hvEnv = () => `<div class="hv-envs"><div class="hv-env">${hvIn("HTTPS_PROXY", "NAME", "Variable name")}<input class="hv-in" type="password" placeholder="saved; type to replace" aria-label="Value"><button class="dbtn quiet small" aria-label="Remove HTTPS_PROXY" data-toast="Removed">&#215;</button></div><div class="hv-env">${hvIn("", "NAME", "Variable name")}<input class="hv-in" type="password" placeholder="value" aria-label="Value"><span></span></div></div>`;
  const hvRuntime = (a, h, saved) => `<h4>Runtime</h4>${hvField("Binary path", `The command Beep starts for ${h.name} on build-box.`, hvIn("", h.prog, "Binary path"), h.diffs?.program)}${hvField(h.home, "Its home and config folder.", hvIn("", h.hdef, h.home))}${hvField("Launch arguments", "Added to every session it starts.", hvIn("", "none", "Launch arguments"), h.diffs?.args)}${hvField("Environment", "Encrypted, never shown again. Put keys in Credentials.", hvEnv())}
    <div class="hv-acts hv-save"><button class="dbtn pri" title="Save for build-box only"${saved === "dirty" ? ' data-toast="Saved for build-box"' : " disabled"}>Save</button><button class="dbtn" title="Save these settings on every machine; offline ones get them when they next connect" data-toast="Saved to 3 machines">Save to all hosts</button><label class="hv-incl" title="Binary paths usually differ between machines"><input type="checkbox"> Include binary path</label>${saved === "dirty" ? '<button class="dbtn quiet">Discard</button>' : ""}<span class="hv-ok">${saved === "all" ? "Saved to 3 machines" : ""}</span></div>`;
  const hvModelChoices = a => a === "claude"
    ? [{ id: "opus", name: "Opus 5.5" }, { id: "sonnet", name: "Sonnet 5.5" }]
    : [{ id: a === "flsh" ? "openrouter:vendor/model-large" : "model-large", name: "Large model" }, { id: a === "flsh" ? "openrouter:vendor/model-fast" : "model-fast", name: "Fast model" }];
  const hvModelId = (agent, value) => hvModelChoices(agent).find(m => m.name === value.trim())?.id ?? value.trim();
  const hvDefaultModel = a => {
    const saved = S.d.harnessDefaults?.[a] || "";
    const draft = S.d.harnessModelDrafts?.[a] ?? saved;
    const models = hvModelChoices(a);
    const display = models.find(m => m.id === draft)?.name ?? draft;
    return hvField("Default model", "Used for new sessions on every machine. Blank uses the harness default.",
      `<div class="hv-default-model" data-harness="${a}"><input class="hv-in" value="${esc(display)}" list="harness-default-models-${a}" aria-label="Default model" placeholder="Harness default" autocomplete="off" spellcheck="false"><datalist id="harness-default-models-${a}">${models.map(m => `<option value="${esc(m.name)}"></option>`).join("")}</datalist><div class="hv-acts"><button class="dbtn pri" data-save-harness-model${draft.trim() === saved ? " disabled" : ""}>Save default model</button><span class="hv-ok" role="status">${S.d.harnessModelSaved === a ? "Default model saved" : ""}</span></div></div>`);
  };
  document.addEventListener("input", event => {
    const row = event.target.closest(".hv-default-model");
    if (!row) return;
    const agent = row.dataset.harness;
    S.d.harnessModelDrafts ??= {};
    S.d.harnessModelDrafts[agent] = hvModelId(agent, event.target.value);
    row.querySelector("[data-save-harness-model]").disabled = hvModelId(agent, event.target.value) === (S.d.harnessDefaults?.[agent] || "");
    row.querySelector('[role="status"]').textContent = "";
    S.d.harnessModelSaved = null;
  });
  document.addEventListener("click", event => {
    const button = event.target.closest("[data-save-harness-model]");
    if (!button || button.disabled) return;
    const row = button.closest(".hv-default-model"), agent = row.dataset.harness;
    S.d.harnessDefaults ??= {};
    S.d.harnessDefaults[agent] = hvModelId(agent, row.querySelector("input").value);
    delete S.d.harnessModelDrafts[agent];
    S.d.harnessModelSaved = agent;
    render();
  });
  const hvModels = a => `<h4>Models and context</h4>${hvDefaultModel(a)}${["codex", "claude", "pi"].includes(a) ? `<div class="hv-note">Limits from this machine's harness. Maximum usable can be smaller than the API limit.</div><div class="hv-note">Overrides apply to new or resumed sessions on every machine, between turns. Blank uses Auto-compact above; off disables Beep's trigger. The harness may compact earlier. Enter 80%, 350000, or off.</div>
    <input class="hv-in" placeholder="Filter models" aria-label="Filter models"><div class="model-context-scroll"><table class="fl-tbl model-context"><thead><tr><th>Model</th><th>Default usable</th><th>Maximum usable</th><th>Compact at</th></tr></thead><tbody>
    <tr><td>model-large</td><td>360,000</td><td>720,000</td><td>${hvIn("350000", "Harness setting", "Compact at for model-large")}</td></tr>
    <tr><td>model-small</td><td>Not reported</td><td>Not reported</td><td>${hvIn("", "Harness setting", "Compact at for model-small")}</td></tr></tbody></table></div><div class="hv-acts"><button class="dbtn small" data-toast="Added">Add model</button><button class="dbtn pri" data-toast="Model overrides saved">Save model overrides</button><button class="dbtn quiet" data-toast="Changes discarded">Discard</button></div>` : '<div class="hv-note">Choose a model from this harness, or enter a model id.</div>'}`;
  const flshOwn = () => {
    const tri = (opts, on, label) => `<span class="fl-seg" role="group" aria-label="${label}">${opts.map(o => `<button class="dchip" aria-pressed="${o === on}">${o}</button>`).join("")}</span>`;
    const quants = `<span class="fl-chips" role="group" aria-label="Allowed quantizations">${["int4", "int8", "fp4", "fp6", "fp8", "fp16", "bf16", "fp32", "unknown"].map(q => `<button class="dchip" aria-pressed="${["fp8", "fp16", "bf16"].includes(q)}">${q}</button>`).join("")}</span>`;
    const models = [["openrouter:vendor/model-large", true, "300000"], ["openrouter:vendor/model-flash", true, ""], ["venice:model-uncensored", false, ""]];
    const DEF = "Model's default trigger";
    return `<h4>OpenRouter routing</h4><div class="hv-note">Applies to every machine.</div>
      ${hvField("Zero data retention", "Only endpoints that keep no prompts.", tri(["Account setting", "Require", "Off"], "Require", "Zero data retention"))}
      ${hvField("Allowed quantizations", "Weight precisions providers may serve.", quants)}
      ${hvField("Sort providers by", "Default: OpenRouter balances price and uptime.", tri(["Default", "Price", "Throughput", "Latency"], "Throughput", "Sort providers by"))}
      ${hvField("Allow fallbacks", "Try other providers when the first ones fail.", tri(["On (OpenRouter default)", "Off"], "On (OpenRouter default)", "Allow fallbacks"))}
      <h4>Auto-compact</h4>
      ${hvField("All models", "Compact once the prompt passes this many tokens.", `<span class="fl-row">${swi(true, "Auto-compact for all models")}${hvIn("", DEF, "Auto-compact threshold")}</span>`)}
      <table class="fl-tbl"><tr><th>Model</th><th>On</th><th>Threshold (tokens)</th><th></th></tr>
      ${models.map(([m, on, t], i) => `<tr><td>${hvIn(m, "provider:model", `Model ${i + 1}`)}</td><td>${swi(on, `Auto-compact for model ${i + 1}`)}</td><td>${hvIn(t, on ? DEF : "Off", `Threshold for model ${i + 1}`)}</td><td><button class="dbtn quiet small" aria-label="Remove ${m}" data-toast="Removed ${m}">&#215;</button></td></tr>`).join("")}
      <tr><td></td><td></td><td></td><td><button class="dbtn small" data-toast="Added">Add model</button></td></tr></table>
      <div class="hv-acts"><button class="dbtn pri" disabled>Save</button></div>`;
  };
  const hvStatusBlock = (a, h, inSet, st) => {
    const m = "build-box", row = html => inSet ? "" : `<div class="hv-stat">${html}</div>`;
    if (h.missing) return row(`Not installed on ${m}.`) + `<div class="hv-acts"><button class="dbtn pri" data-toast="Installing ${h.name} on ${m}">Install on ${m}</button><button class="dbtn quiet" data-toast="Copied: ${h.install}">Copy install command</button></div>`;
    let out = "";
    if (st === "token") out += row(`<span class="ddot"></span>Signs in with the token in Agent accounts (…a91f)`) + `<div class="hv-acts"><button class="dbtn" ${dsetGo("acct")}>Agent accounts</button></div>`;
    else if (st === "out" || h.out) {
      out += row(`<span class="ddot" style="background:var(--d-coral)"></span>Not signed in on ${m}`);
      out += a === "claude" || a === "codex" ? `<div class="hv-acts"><button class="dbtn pri" ${dsetGo("acct")}>Sign in</button></div>` : `<div class="hv-note">Run ${a} on ${m} and sign in there.</div>`;
    } else if (h.unknown) {
      const why = `${h.name} does not say whether it is signed in. Run it on ${m} to sign in.`;
      out += inSet ? `<div class="hv-note">${why}</div>` : row(why);
    } else out += row(`<span class="ddot"></span>${h.status}`);
    // Its ACP adapter, which beep-node installs itself (#425).
    if (st === "adapting") out += `<div class="hv-note">Installing the ${h.name} adapter</div>`;
    else if (st === "adapter") out += `<div class="hv-note bad">Could not install the ${h.name} adapter: could not download Node.js: no curl or wget</div><div class="hv-acts"><button class="dbtn" data-toast="Trying again on ${m}">Try again</button></div>`;
    else if (st === "stale") out += `<div class="hv-note">Restart beep-node to use it.</div>`;
    if (h.latest) out += `<div class="hv-acts"><button class="dbtn"${st === "updating" ? " disabled" : ` data-toast="Updating ${h.name} on ${m}"`}>${st === "updating" ? "Updating…" : `Update to v${h.latest}`}</button></div>`;
    return out;
  };
  const failLine = (t, more) => `<div class="fb-err" role="alert"><div class="fb-line"><span>${t}</span><button class="fb-x" aria-label="Dismiss" data-toast="Dismissed">&#215;</button></div>${more ? `<details class="fb-more"><summary>Details</summary><pre>${more}</pre></details>` : ""}</div>`;
  // A coding agent's page in Settings. `k` is the agent, then its state: acct-claude, acct-claude-off.
  const harnessPage = v => {
    const [, a, st = ""] = v.split("-");
    const bar = `<div class="hv-bar">${hvPicker()}<span class="sp"></span><button class="dbtn quiet" title="Check the versions again" data-toast="Checked build-box again">Checked just now</button><button class="dbtn"${a === "claude" ? ' data-toast="Updating 1"' : ' disabled title="Everything is up to date"'}>Update all</button></div>`;
    const wrap = inner => `<div class="cc-view hv-view in-set">${inner}</div>`;
    if (st === "none") return `<div class="cc-view hv-view in-set"><div class="hv-bar">${hvPicker().replace(/aria-pressed="true"/, 'aria-pressed="false"')}<span class="sp"></span></div><div class="cc-panel cc-empty"><b>No machine online</b><span>Coding agents show here for machines that are online.</span></div></div>`;
    if (st === "asking") return wrap(`<div class="cc-panel cc-empty"><b>Asking build-box…</b></div>`);
    if (st === "gone") return wrap(`<div class="cc-panel cc-empty"><b>build-box does not report ${HV[a].name}</b></div>`);
    const h = HV[a], off = st === "off";
    const on = !h.missing && !off;
    let status = h.status, bad = h.bad;
    if (h.missing) status = "Not installed";
    if (off) status = "Turned off: Auto and the start form skip it";
    if (st === "token") status = "Signs in with Beep's token …a91f";
    if (st === "out") { status = "Not signed in on this machine"; bad = true; }
    if (st === "adapting") status = `Installing the ${h.name} adapter`;
    if (st === "adapter") { status = `Could not install the ${h.name} adapter`; bad = true; }
    const end = h.missing ? "<span></span><span></span>" : `<span class="on">${on ? "On" : "Off"} for build-box</span>${hvSwitch(a, on)}`;
    const summary = `<div class="hv-sum${!h.missing && !on ? " off" : ""}">${tile(a)}<span class="hv-name">${nameLine(h.name, h.ver, h.latest)}${statusSmall(status, bad)}</span>${end}</div>`;
    const banner = st === "fail" ? failLine("Could not update Claude Code on build-box: npm exited with code 1", "$ npm i -g @anthropic-ai/claude-code\nnpm ERR! code EACCES") : "";
    const detail = `<div class="hv-d">${hvStatusBlock(a, h, true, st)}${h.missing ? "" : hvRuntime(a, h, st === "dirty" ? "dirty" : st === "saved" ? "all" : "")}</div>`;
    const shared = `<section class="hv-scope hv-d"><h3>All machines</h3><div class="hv-note">These settings apply on every machine.</div>${a === "flsh" || a === "gemini" || a === "hermes" ? "" : hvField("Auto-compact", a === "opencode" ? "Compact when the conversation nears the window." : "Compact at this share of the window, or this many tokens.", `<span class="fl-row">${swi(true, `Auto-compact for ${h.name}`)}${a === "opencode" ? "" : hvIn("", "80% or 350000", "Auto-compact threshold")}</span>`)}<div class="hv-acts"><button class="dbtn pri" disabled>Save auto-compact</button></div>${a === "flsh" ? flshOwn() : ""}<div class="hv-note">Model choices and reported limits come from build-box.</div>${hvModels(a)}</section>`;
    return wrap(shared + `<section class="hv-scope">${bar}${banner}${summary}${detail}</section>`);
  };
  // The window's own Coding agents view: the list on the left, the picked agent's page on the right.
  const providers = (v, failBanner) => {
    const sel = v === "claude" || v === "signin" || v === "token" ? "claude" : v === "missing" ? "gemini" : v === "off" ? "pi" : v === "opencode" ? "opencode" : v === "flsh" ? "flsh" : v === "hermes" ? "hermes" : "codex";
    const st = v === "signin" ? "out" : v === "token" ? "token" : "";
    const rows = ["claude", "codex", "gemini", "pi", "opencode", "flsh", "hermes"].map(a => {
      const h = HV[a], isOff = v === "off" && a === "pi", on = !h.missing && !isOff;
      let status = h.missing ? "Not installed" : isOff ? "Turned off: Auto and the start form skip it" : a === "claude" && v === "signin" ? "Not signed in on this machine" : a === "claude" && v === "token" ? "Signs in with Beep's token …a91f" : h.list;
      const bad = h.bad || (a === "claude" && v === "signin");
      return `<div class="hv-it${h.missing || on ? "" : " off"}" role="button" tabindex="0" aria-current="${a === sel}" data-dact="dset:pv:${a === "gemini" ? "missing" : a === "pi" ? "off" : a}">${tile(a)}<span class="hv-name">${nameLine(h.name, h.ver, h.latest)}${statusSmall(status, bad)}</span>${h.missing ? "<span></span>" : hvSwitch(a, on)}</div>`;
    }).join("");
    const h = HV[sel];
    const detail = `<div class="hv-d"><h3>${tile(sel)}${h.name}<span class="ver">${h.missing ? "" : h.latest ? `v${h.ver} · v${h.latest} out` : `v${h.ver}`}</span></h3><h4>Status</h4>${hvStatusBlock(sel, h, false, st)}${h.missing ? "" : hvRuntime(sel, h, "")}${sel === "flsh" ? flshOwn() : ""}</div>`;
    const updBtn = v === "update" ? `<button class="dbtn" disabled>Updating 2…</button>` : `<button class="dbtn" data-dact="dset:pv:update">Update all</button>`;
    const check = `<button class="dbtn quiet" title="Check the versions again" data-toast="Checked build-box again">Checked 4 min ago</button>`;
    const head = `<div class="cc-top"><h2>Coding agents</h2><small>What each machine can run</small><span class="tools">${hvPicker()}${updBtn}${check}</span></div>`;
    const body = v === "none" ? `<div class="cc-panel cc-empty"><b>No machine online</b><span>Coding agents show here for machines that are online.</span></div>`
      : `${v === "fail" ? failBanner : ""}<div class="hv"><div class="hv-list">${rows}</div>${detail}</div>`;
    return ccWin(ccSide(""), `${head}<div class="cc-view hv-view">${body}</div>`);
  };

  // What the left pane lists, and which page a variant shows.
  return { HV, hvSwitch, nameLine, statusSmall, hvPicker, hvIn, hvField, hvEnv, hvRuntime, hvModelChoices, hvModelId, hvDefaultModel, hvModels, flshOwn, hvStatusBlock, failLine, harnessPage, providers };
})();
