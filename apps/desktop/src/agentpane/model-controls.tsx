import { useEffect, useState } from "react";
import type { Animation } from "./model3d.ts";
import { clipTime } from "./media-state.ts";
import { ICON, Icon } from "../ui/icons.tsx";
import { Button, Picker } from "../ui/kit.tsx";

interface Playback { clip: number; playing: boolean; loop: boolean; time: number }
const playback = (animation: Animation): Playback => ({ clip: animation.clip, playing: animation.playing, loop: animation.loop, time: animation.time() });

export function ModelControls({ animation }: { animation: Animation }) {
  const [state, setState] = useState(() => playback(animation));
  useEffect(() => {
    const update = () => setState(playback(animation));
    animation.onchange = update;
    update();
    if (!matchMedia("(prefers-reduced-motion: reduce)").matches) animation.play(true);
    return () => { if (animation.onchange === update) animation.onchange = () => {}; };
  }, [animation]);
  const clip = animation.clips[state.clip], end = clip?.duration ?? 0;
  const label = state.playing ? "Pause" : "Play";
  return <><Button className="ibtn" data-play="1" title={label} aria-label={label} onClick={() => animation.play(!animation.playing)} dangerouslySetInnerHTML={{ __html: state.playing ? ICON.pause : ICON.play }} />{animation.clips.length > 1 && <Picker label="Clip" options={animation.clips.map((item, index) => ({ id: String(index), label: item.name }))} value={String(state.clip)} onChange={index => animation.select(Number(index))} />}<input type="range" min="0" step="any" aria-label="Time" max={end} value={state.time} onInput={event => { animation.play(false); animation.seek(Number(event.currentTarget.value)); }} /><span className="time">{clipTime(state.time)} / {clipTime(end)}</span><Button className={`ibtn${state.loop ? " on" : ""}`} data-loop="1" aria-label="Loop" title="Loop" aria-pressed={state.loop} onClick={() => animation.setLoop(!animation.loop)}><Icon name="loop" /></Button></>;
}
