import { Fragment, type ReactNode } from "react";
import { LabelGroup, Row } from "./split-view.tsx";
import type { LabelFilter, SplitItem } from "./split.ts";

interface Props {
  error: string | null;
  items: readonly SplitItem[];
  filter: LabelFilter;
  controls: ReactNode;
  start: ReactNode;
  selection: (id: string) => boolean | null;
  pick: (id: string, checked: boolean) => void;
  openTile: (key: string) => void;
}

export function ProjectIssues({ error, items, filter, controls, start, selection, pick, openTile }: Props) {
  return <>
    {error && <div className="cc-note"><span className="ddot bad"/><span className="cc-err">Could not refresh the project: {error}</span></div>}
    {controls}
    <LabelGroup filter={filter}/>
    <div className="pj-rows">{items.map(item => {
      const checked = item.start ? selection(item.key) : null;
      const row = <Row item={item} picked={null} filter={filter} nav={false} openTile={openTile}/>;
      return checked === null ? <Fragment key={item.key}>{row}</Fragment> :
        <div className="pj-prow" key={item.key}><label className="pick"><input type="checkbox" aria-label="Pick this issue" checked={checked}
          onClick={event => event.stopPropagation()} onChange={event => pick(item.key, event.currentTarget.checked)}/></label>{row}</div>;
    })}</div>
    {start}
  </>;
}
