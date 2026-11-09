import { Fragment, useLayoutEffect, useRef } from "react";
import type { Kid } from "../ui/dom.ts";
import { patchChildren } from "../ui/patch.ts";
import type { LabelFilter, SplitGroup, SplitItem } from "./split.ts";

export function ProjectNavigation({ groups, picked, openTile }: { groups: readonly SplitGroup[]; picked: string; openTile: (key: string) => void }) {
  return <>{groups.map((group, index) => <Fragment key={`${index}:${group.title}`}>
    {!group.title && index > 0 && <hr/>}
    {group.title && <h4>{group.title}{group.items.length > 0 && <span className="n">{group.items.length}</span>}</h4>}
    {group.note && <div className="note">{group.note}</div>}
    {group.controls && <Legacy node={group.controls}/>}
    {group.labels && group.labels.all.length > 0 && <LabelGroup filter={group.labels}/>}
    {group.items.map(item => <Row key={item.key} item={item} picked={picked} filter={group.labels} nav={!group.title} openTile={openTile}/>)}
  </Fragment>)}</>;
}

export function LabelGroup({ filter }: { filter: LabelFilter }) {
  return <div className="pj-labels">{filter.all.map(label => <Label key={label} label={label} filter={filter} button/>)}</div>;
}

export function Row({ item, picked, filter, nav, openTile }: { item: SplitItem; picked: string | null; filter?: LabelFilter; nav: boolean; openTile: (key: string) => void }) {
  return <button type="button" className={`pj-li${nav ? " nav" : ""}`} title={item.title} aria-current={item.key === picked ? "true" : undefined}
    onClick={() => item.open ? item.open() : openTile(item.key)} onContextMenu={item.menu ? e => item.menu?.(e.nativeEvent) : undefined}>
    {nav ? <><span className="t"><b>{item.title}</b></span><span className="c">{item.sub}</span></> : <>
      <span className="lead"><Legacy node={item.lead}/></span><span className="t"><b>{item.title}</b><span>{item.sub}{item.tags?.map(label => filter ? <Label key={label} label={label} filter={filter}/> : <i className="pj-tag" key={label}>{label}</i>)}</span></span>
      <span className="r"><Legacy node={item.right}/></span>
    </>}
  </button>;
}

function Label({ label, filter, button = false }: { label: string; filter: LabelFilter; button?: boolean }) {
  const cls = `pj-tag lab${label === filter.on ? " on" : ""}`;
  const pick = (event: React.MouseEvent) => { event.stopPropagation(); filter.pick(label); };
  return button ? <button type="button" className={cls} onClick={pick}>{label}</button> : <i className={cls} onClick={pick}>{label}</i>;
}

/// Keep existing controls until their feature is migrated, without detaching unchanged nodes.
export function Legacy({ node, flexItem = false }: { node: Kid; flexItem?: boolean }) {
  const slot = useRef<HTMLSpanElement>(null);
  useLayoutEffect(() => {
    const target = slot.current;
    if (!target) return;
    if (node instanceof Element) {
      if (flexItem && node instanceof HTMLElement) node.style.flex = "none";
      patchChildren(target, [node]);
    } else if (node instanceof Node) {
      if (!target.firstChild?.isEqualNode(node) || target.childNodes.length !== 1) target.replaceChildren(node);
    } else {
      const text = typeof node === "string" ? node : "";
      if (target.textContent !== text) target.textContent = text;
    }
  }, [node, flexItem]);
  return <span ref={slot} style={{ display: "contents" }}/>;
}

export function ProjectStatus({ cells, openTile }: { cells: readonly { label: string; text: string; key: string; bad?: boolean }[]; openTile: (key: string) => void }) {
  return <>{cells.filter(cell => cell.label && cell.text).map(cell => <button type="button" className="m" key={cell.key} onClick={() => openTile(cell.key)}><b>{cell.label}</b><span className={cell.bad ? "bad" : ""}>{cell.text}</span></button>)}</>;
}
