import { cap } from "../chat/tools.ts";
import { modelPrice, THINKING, type Models, type Thinking } from "./model.ts";

export function ModelSettingsView({ model, failed, busy, error, pick, auto, thinking }: { model: Models | null; failed: boolean; busy: boolean; error: string; pick(id: string): void; auto(enabled: boolean): void; thinking(level: Thinking): void }) {
  if (!model) return failed ? <div className="dmuted">Could not load your models.</div> : <div className="dskel-rows"><div className="dskel" style={{ width: "78%" }} /><div className="dskel" style={{ width: "56%" }} /></div>;
  const selected = model.choices.find(choice => choice.id === model.selected);
  return <><div className="dtoggle"><span><b>Model</b><small>{selected ? modelPrice(selected) : "Your turns use the server's default"}</small></span><select id="setModel" aria-label="Model" value={model.selected} disabled={busy} onChange={e => pick(e.currentTarget.value)}>{model.choices.map(choice => <option key={choice.id} value={choice.id}>{choice.name}</option>)}</select></div>
    {model.auto_available && <div className="dtoggle"><span><b>Auto thinking</b><small>{model.auto ? "Beep picks the thinking for each prompt" : "Every prompt thinks as set below"}</small></span><button type="button" className="dswitch" role="switch" id="setAuto" aria-label="Auto thinking" aria-checked={model.auto} disabled={busy} onClick={() => auto(!model.auto)} /></div>}
    <div className="dfield"><span>Thinking{model.auto ? " (Auto picks it per prompt)" : ""}</span><div className="dchips">{THINKING.map(level => <button key={level} className="dchip" data-setlevel={level} aria-pressed={!model.auto && level === model.thinking} disabled={model.auto || busy} onClick={() => thinking(level)}>{cap(level)}</button>)}</div></div><div className="derr" id="setModelErr">{error}</div>
  </>;
}
