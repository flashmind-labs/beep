import { Fragment, type ReactNode } from "react";
import { LabelGroup, Row } from "./split-view.tsx";
import type { LabelFilter, SplitItem } from "./split.tsx";
import { Checkbox } from "../../ui/kit.tsx";

interface Props {
  items: readonly SplitItem[];
  filter: LabelFilter;
  /// The team's All, Mine and Unassigned chips; null without a team.
  owners: ReactNode;
  controls: ReactNode;
  start: ReactNode;
  selection: (id: string) => boolean | null;
  pick: (id: string, checked: boolean) => void;
  openTile: (key: string) => void;
}

export function ProjectIssues({ items, filter, owners, controls, start, selection, pick, openTile }: Props) {
  return <>
    {controls}
    {owners}
    <LabelGroup filter={filter}/>
    <div className="pj-rows">{items.map(item => {
      const checked = item.start ? selection(item.key) : null;
      const row = <Row item={item} picked={null} filter={filter} nav={false} openTile={openTile}/>;
      return checked === null ? <Fragment key={item.key}>{row}</Fragment> :
        <div className="pj-prow" key={item.key}><label className="pick"><Checkbox label="Pick this issue" checked={checked} contained
          change={value => pick(item.key, value)} /></label>{row}</div>;
    })}</div>
    {start}
  </>;
}
