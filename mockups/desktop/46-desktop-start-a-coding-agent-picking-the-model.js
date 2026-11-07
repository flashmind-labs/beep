// ---- Desktop: Start a coding agent, picking the model ----
// Mirrors drawModel and loadModels in agentnew.ts and agentmodels.ts. A Model field follows Agent once
// an agent and a machine are known (under Auto, the machine Auto would pick). The machine is asked
// with ListAgentModels; while it answers the field shows a spinner. A dropdown lists the models,
// "Default (<current>)" first; an agent that lists none gets a box to type a name, its default as the
// placeholder; it is hidden for Any, for an agent that cannot switch, and for a machine whose
// beep-node is too old. flsh lists its models with `flsh models` and names its default in config.toml.
(() => {
  const st = document.createElement("style");
  st.textContent = `.ag-model { position: relative }
.ag-model .box.sel { display: flex; align-items: center; justify-content: space-between; gap: 8px; border-color: var(--d-emerald) }
.ag-model .box.sel svg { flex: none; color: var(--d-muted) }
.ag-menu { position: absolute; top: calc(100% + 4px); left: 0; right: 0; z-index: 6; max-height: 210px; overflow-y: auto; padding: 4px 0; border: 1px solid var(--d-border); border-radius: 8px; background: #0f1011; box-shadow: 0 10px 30px rgba(0,0,0,.45); scrollbar-width: thin }
.ag-menu button { all: unset; box-sizing: border-box; display: flex; align-items: baseline; gap: 8px; width: 100%; padding: 5px 11px; font-size: 13px; cursor: pointer }
.ag-menu button small { color: var(--d-muted); font-size: 11.5px }
.ag-menu button:hover { background: rgba(255,255,255,.04) }
.ag-menu button.on { background: rgba(94,106,210, .14) }
.ag-menu .mono { font-family: var(--mono); font-size: 12.5px }`;
  document.head.append(st);

  const caret = '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M6 9l6 6 6-6"/></svg>';
  const current = "openai:gpt-test";
  const models = ["anthropic:model-large", "anthropic:model-small", "ollama:local-model", "openai:gpt-test", "openai:gpt-mini", "openrouter:vendor/model-a", "openrouter:vendor/model-b"];
  const field = (v) => {
    if (v === "asking") return `<div class="ag-f"><span>Model</span><span class="note"><span class="dspin"></span> Asking the machine which models it has…</span></div>`;
    if (v === "typed") return `<div class="ag-f"><span>Model</span><div class="box mono ph">Default (${current})</div></div>`;
    const menu = v === "open" ? `<div class="ag-menu" role="listbox">
        <button type="button" role="option" aria-selected="true" class="on" data-toast="Starts on flsh's default">Default (${current})</button>
        ${models.map(m => `<button type="button" role="option" aria-selected="false" class="mono" data-toast="Starts flsh with --model=${m}">${m}${m === current ? " <small>default</small>" : ""}</button>`).join("")}
      </div>` : "";
    return `<div class="ag-f ag-model"><span>Model</span><div class="box sel">Default (${current})${caret}</div>${menu}</div>`;
  };

  const base = D.dAgentNew;
  D.dAgentNew = (...a) => {
    const v = S.d.agm;
    if (!["open", "asking", "typed"].includes(v)) return base(...a);
    // The form on win-box, which has Claude Code and flsh, with flsh picked.
    const keep = S.d.ag;
    S.d.ag = "chosen";
    let html = base(...a);
    S.d.ag = keep;
    html = html
      .replace(/aria-pressed="true"([^>]*)>build-box</, 'aria-pressed="false"$1>build-box<')
      .replace(/aria-pressed="false"([^>]*)>win-box</, 'aria-pressed="true"$1>win-box<')
      .replace("Has GPU, CUDA, Docker", "Has GPU, Blender, Unreal")
      .replace(/(<span>Agent<\/span><div class="dchips">)<button class="dchip" aria-pressed="true"([^>]*)>Any<\/button>[\s\S]*?(<\/div><\/div>)/,
        (_, open, attrs, close) => `${open}<button class="dchip" aria-pressed="false"${attrs}>Any</button><button class="dchip" aria-pressed="false">Claude Code</button><button class="dchip" aria-pressed="true">flsh</button>${close}\n        ${field(v)}`);
    return html;
  };
  VARIANTS.dAgentNew = VARIANTS.dAgentNew.map(([l, f, on]) => [l, () => { S.d.agm = ""; f(); }, () => !S.d.agm && on()]);
  VARIANTS.dAgentNew.push(
    ["Model list open (flsh)", () => { S.d.agm = "open"; render(); }, () => S.d.agm === "open"],
    ["Asking for models", () => { S.d.agm = "asking"; render(); }, () => S.d.agm === "asking"],
    ["Model typed (no list)", () => { S.d.agm = "typed"; render(); }, () => S.d.agm === "typed"]);
})();
