import { useEffect, useRef, useState, type CSSProperties, type KeyboardEvent } from "react";
import { useStore } from "zustand";
import { subscribeChanges, topic } from "../ui/mesh-events.ts";
import { onShownChange, pageShown } from "../ui/active.ts";
import { invoke, listen } from "../ui/tauri.ts";
import { createSpriteDriver, createSpriteScript, type SpriteDriver } from "./sprite.ts";
import { clipBytes, connectPluginSlots, connectPluginTopics, connectPluginVisibility, connectPlugins, followPlugin, instanceBook, instanceId, placementsFor, pluginsStore, publishUi, sendPluginEvent, startDraft, typeInto } from "./plugins-state.ts";
import type { CallbackId, ComposerShown, Placement, PluginAction, PluginNode, PluginPosition, SlotInstance } from "./plugins-state.ts";

const bridge = { invoke: (cmd: string, args?: Record<string, unknown>) => invoke(cmd, args), listen };
void connectPlugins(bridge);
void connectPluginTopics({
  ...bridge,
  follow: (wanted, receive) => subscribeChanges(
    topic(wanted.name, wanted.key),
    { deliver: "each", receive: event => receive(event.machine_id, event.data ?? null) },
    { machine: wanted.machine },
  ),
});
connectPluginSlots({ invoke: (cmd, args) => invoke(cmd, args) });
connectPluginVisibility({ invoke: (cmd, args) => invoke(cmd, args), shown: pageShown, onChange: onShownChange });

/// The sprites on this page. They play only while the window shows and the user has not asked for less motion.
const sprites = new Set<SpriteDriver>();
const reducedMotion = typeof matchMedia === "function" ? matchMedia("(prefers-reduced-motion: reduce)") : null;
const canAnimate = () => pageShown() && !(reducedMotion?.matches ?? false);
const syncSprites = () => { const on = canAnimate(); for (const sprite of sprites) sprite.setActive(on); };
onShownChange(syncSprites);
reducedMotion?.addEventListener("change", syncSprites);

/// The longest session title a row tells plugins, in bytes.
const MAX_TITLE_BYTES = 256;
/// Input text is held to this many characters, which fits the host's 4 KB in any encoding.
const MAX_INPUT_CHARS = 1024;

/// The key of a session's instance: the machine and the session, which the page and host show the same way.
const sessionKey = (machine: string, session: string) => `${machine}:${session}`;

/// Publishes `ui.composer` and the composer slot's instance while the session's message field is on the page.
export function ComposerPresence({ machine, session }: ComposerShown) {
  useEffect(() => {
    publishUi({ name: "ui.composer", data: { machine, session } });
    instanceBook.show({ slot: "composer", key: "composer", context: { machine, session } });
    return () => {
      publishUi({ name: "ui.composer", data: null });
      instanceBook.hide("composer", "composer");
    };
  }, [machine, session]);
  return null;
}

/// Where a placement sits on the slot's edge: `align` picks the side, `offset`
/// the inset from it, `sink` how far it drops below the edge.
function edgeStyle({ align, offset, sink }: Extract<PluginPosition, { kind: "edge" }>): CSSProperties {
  const drop = `translateY(${sink}px)`;
  if (align === "start") return { left: offset, transform: drop };
  if (align === "end") return { right: offset, transform: drop };
  return { left: `calc(50% + ${offset}px)`, transform: `translateX(-50%) ${drop}` };
}

function send(plugin: string, id: CallbackId, action: PluginAction) {
  sendPluginEvent({ plugin, id, action });
}

function NodeView({ node, plugin }: { node: PluginNode; plugin: string }) {
  switch (node.kind) {
    case "image":
      return <ImageView node={node} />;
    case "sprite":
      return <SpriteView node={node} />;
    case "text":
      return <span className={node.tone === "normal" ? undefined : `plugintext ${node.tone}`}>{node.text}</span>;
    case "row":
      return <div className="pluginrow" style={{ gap: node.gap }}>{children(node.children, plugin)}</div>;
    case "button":
      return <button type="button" className="dbtn small pluginctl" aria-label={node.label} onClick={() => send(plugin, node.onPress, { kind: "press" })}>{node.children.length > 0 ? children(node.children, plugin) : node.label}</button>;
    case "toggle":
      return <button type="button" className="dswitch pluginctl" role="switch" aria-checked={node.value} aria-label={node.label} onClick={() => send(plugin, node.onChange, { kind: "toggle", value: !node.value })} />;
    case "input":
      return <PluginInput node={node} plugin={plugin} />;
  }
}

/// The URL of an image the page has fetched, or null while it is on its way.
const useAssetUrl = (hash: string) => useStore(pluginsStore, state => state.assets.get(hash) ?? null);

function ImageView({ node }: { node: Extract<PluginNode, { kind: "image" }> }) {
  const url = useAssetUrl(node.asset.hash);
  if (url === null) return null;
  return <img src={url} width={node.width} height={node.height} alt="" draggable={false} style={node.pixelated ? { imageRendering: "pixelated" } : undefined} />;
}

/// A sprite stacks its frames and shows one. The driver changes which, from timers on this page, so the
/// plugin is never woken for a frame. Until every frame has arrived nothing shows.
function SpriteView({ node }: { node: Extract<PluginNode, { kind: "sprite" }> }) {
  const urls = useStore(pluginsStore, state => state.assets);
  const box = useRef<HTMLSpanElement>(null);
  const ready = node.frames.every(frame => urls.has(frame.hash));
  const script = JSON.stringify([node.rest, node.motions]);

  useEffect(() => {
    const element = box.current;
    if (!ready || element === null) return;

    const images = Array.from(element.children);
    const show = (frame: number) => images.forEach((image, index) => image.toggleAttribute("data-on", index === frame));
    const driver = createSpriteDriver(createSpriteScript(node.motions, node.rest, Math.random), node.rest, {
      now: () => performance.now(),
      setFrame: show,
      setTimer: (fn, ms) => window.setTimeout(fn, ms),
      clearTimer: id => window.clearTimeout(id),
    });
    show(node.rest);
    sprites.add(driver);
    driver.setActive(canAnimate());
    return () => { sprites.delete(driver); driver.stop(); };
    // The script's text stands for its motions.
  }, [ready, script]);

  if (!ready) return null;
  return <span ref={box} className="pluginsprite" style={{ width: node.width, height: node.height, imageRendering: node.pixelated ? "pixelated" : undefined }}>
    {node.frames.map((frame, index) => <img key={index} src={urls.get(frame.hash)} width={node.width} height={node.height} alt="" draggable={false} data-frame={index} data-on={index === node.rest ? "" : undefined} />)}
  </span>;
}

const children = (nodes: readonly PluginNode[], plugin: string) => nodes.map((child, index) => <NodeView key={index} node={child} plugin={plugin} />);

/// A text field that shows what is typed at once and follows the plugin's `value` when that changes. A late
/// echo of older text does not replace newer typing.
function PluginInput({ node, plugin }: { node: Extract<PluginNode, { kind: "input" }>; plugin: string }) {
  const [draft, setDraft] = useState(() => startDraft(node.value));
  const { text } = draft;
  useEffect(() => setDraft(current => followPlugin(current, node.value)), [node.value]);

  const keys = (event: KeyboardEvent<HTMLInputElement>) => {
    // The app's own keys stay out of a field, except its modifier shortcuts.
    if (!event.metaKey && !event.ctrlKey) event.stopPropagation();
    if (event.key === "Escape") event.currentTarget.blur();
    if (event.key === "Enter" && !event.nativeEvent.isComposing && node.onSubmit !== null) send(plugin, node.onSubmit, { kind: "text", value: text });
  };

  return <input type="text" className="pluginin pluginctl" aria-label={node.label} placeholder={node.placeholder} maxLength={MAX_INPUT_CHARS} value={text} spellCheck={false} autoComplete="off"
    onKeyDown={keys}
    onChange={event => {
      const { value } = event.currentTarget;
      setDraft(current => typeInto(current, value));
      if (node.onChange !== null) send(plugin, node.onChange, { kind: "text", value });
    }} />;
}

const hasControls = (node: PluginNode): boolean => {
  switch (node.kind) {
    case "image": case "sprite": case "text": return false;
    case "row": return node.children.some(hasControls);
    case "button": case "toggle": case "input": return true;
  }
};

/// A zero-height line in the page's flow, for the slot that stands on an edge. What plugins place stands on it, over what is above.
/// It takes no pointer except on controls, and hides from screen readers unless it holds some.
export function PluginSlot({ name }: { name: "composer" }) {
  const placements = useStore(pluginsStore, state => state.placements);
  const here = placementsFor(placements, name);
  if (here.length === 0) return null;

  return <div className="pluginslot" aria-hidden={here.some(placement => hasControls(placement.node)) ? undefined : true}>
    {here.map((placement, index) => <div className="pluginplace" key={`${placement.plugin}:${index}`} style={placement.position.kind === "edge" ? edgeStyle(placement.position) : undefined}><NodeView node={placement.node} plugin={placement.plugin} /></div>)}
  </div>;
}

const NONE: readonly Placement[] = [];

/// One instance of a slot in the page's flow: it tells plugins the slot is here, with this context, while it
/// is mounted, and draws what they placed for it. It draws no element while there is nothing to draw, so
/// the layout is as it was without plugins.
function PluginHere({ instance }: { instance: SlotInstance }) {
  const { slot, key } = instance;
  const context = JSON.stringify(instance.context);
  // The context's text stands for the instance's own object.
  useEffect(() => {
    const shown = instanceBook.show(instance);
    return () => { if (shown) instanceBook.hide(slot, key); };
  }, [slot, key, context]);

  const placements = useStore(pluginsStore, state => state.index.get(instanceId(slot, key)) ?? NONE);
  if (placements.length === 0) return null;

  return <span className="pluginhere" data-plugin-slot={slot}>{placements.map((placement, index) => <NodeView key={`${placement.plugin}:${index}`} node={placement.node} plugin={placement.plugin} />)}</span>;
}

/// The `session.row` slot of a session's row in the session list: after the title, before the state.
export function SessionRowSlot({ machine, session, title }: { machine: string; session: string; title: string }) {
  return <PluginHere instance={{ slot: "session.row", key: sessionKey(machine, session), context: { machine, session, title: clipBytes(title, MAX_TITLE_BYTES) } }} />;
}

/// The `session.header` slot of the open session's header.
export function SessionHeaderSlot({ machine, session }: { machine: string; session: string }) {
  return <PluginHere instance={{ slot: "session.header", key: sessionKey(machine, session), context: { machine, session } }} />;
}
