import type { CSSProperties } from "react";
import { useStore } from "zustand";
import { invoke, listen } from "../ui/tauri.ts";
import { connectPlugins, placementsFor, pluginsStore } from "./plugins-state.ts";
import type { Placement, PluginNode, PluginSlotName } from "./plugins-state.ts";

void connectPlugins({ invoke: cmd => invoke(cmd), listen });

/// Where a placement sits on the slot's edge: `align` picks the side, `offset`
/// the inset from it, `sink` how far it drops below the edge.
function placementStyle({ align, offset, sink }: Placement): CSSProperties {
  const drop = `translateY(${sink}px)`;
  if (align === "start") return { left: offset, transform: drop };
  if (align === "end") return { right: offset, transform: drop };
  return { left: `calc(50% + ${offset}px)`, transform: `translateX(-50%) ${drop}` };
}

function NodeView({ node }: { node: PluginNode }) {
  switch (node.kind) {
    case "image":
      return <img src={node.src} width={node.width} height={node.height} alt="" draggable={false} style={node.pixelated ? { imageRendering: "pixelated" } : undefined} />;
    case "text":
      return <span>{node.text}</span>;
    case "row":
      return <div className="pluginrow" style={{ gap: node.gap }}>{node.children.map((child, index) => <NodeView key={index} node={child} />)}</div>;
  }
}

/// A zero-height line in the page's flow. What plugins place stands on it, over what is above.
export function PluginSlot({ name }: { name: PluginSlotName }) {
  const placements = useStore(pluginsStore, state => state.placements);
  const here = placementsFor(placements, name);
  if (here.length === 0) return null;

  return <div className="pluginslot" aria-hidden="true">
    {here.map((placement, index) => <div className="pluginplace" key={`${placement.plugin}:${index}`} style={placementStyle(placement)}><NodeView node={placement.node} /></div>)}
  </div>;
}
