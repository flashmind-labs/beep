// A chunk's rows, unified or side by side, with the comments on each line
// under it. A click on a line's number opens a new comment there.
import { Fragment, type ReactNode } from "react";
import { DiffGapControls } from "./diff-body-controls.tsx";
import { lineKey, type ContextEdit, type DiffRow, type ReviewComment, type SplitCell, type SplitRow } from "./diff-state.ts";

type Side = ReviewComment["side"];
/// Opens a comment on `line` of `side`.
type Gutter = (side: Side, line: number) => void;

interface RowsProps {
  rows: readonly (DiffRow | SplitRow)[];
  comments: ReadonlyMap<string, readonly ReviewComment[]>;
  readOnly: boolean;
  gutter: Gutter;
  /// Shows more of the lines left out around a hunk, or hides them again.
  edit: (edits: readonly ContextEdit[]) => void;
  comment: (comment: ReviewComment) => ReactNode;
}

export function DiffRows({ rows, comments, readOnly, gutter, edit, comment }: RowsProps) {
  const open = readOnly ? null : gutter;
  const under = (keys: (string | null)[]) => keys.flatMap(key => key ? (comments.get(key) ?? []).map(comment) : []);

  return <>{rows.map((row, index) => {
    if (row.kind === "hunk") return <div key={index} className="dv-l hunk"><DiffGapControls row={row} edit={edit}/><span className="dv-c">{row.text}</span></div>;

    if (row.kind === "pair") {
      const { left, right } = row;
      // On an unchanged line the left cell takes its comment on the new side.
      const leftAt: [Side, number] | null = left?.k === "-" ? ["old", left.num] : left && right ? ["new", right.num] : null;
      return <Fragment key={index}>
        <div className="dv-pair"><Cell cell={left} at={leftAt} open={open}/><Cell cell={right} at={right ? ["new", right.num] : null} open={open}/></div>
        {under([left?.k === "-" ? lineKey("old", left.num) : null, right ? lineKey("new", right.num) : null])}
      </Fragment>;
    }

    const at: [Side, number] | null = row.k === "-" ? (row.old === null ? null : ["old", row.old]) : row.new === null ? null : ["new", row.new];
    const cls = row.k === "+" ? " add" : row.k === "-" ? " del" : "";
    return <Fragment key={index}>
      <div className={`dv-l${cls}`}>
        <span className="dv-g" onClick={open && at ? () => open(...at) : undefined}><span>{row.old ?? ""}</span><span>{row.new ?? ""}</span></span>
        <TextCell text={row.text} eol={row.eol}/>
      </div>
      {under([at ? lineKey(...at) : null])}
    </Fragment>;
  })}</>;
}

function Cell({ cell, at, open }: { cell: SplitCell | null; at: [Side, number] | null; open: Gutter | null }) {
  if (!cell) return <div className="dv-l none"><span className="dv-g"><span/></span><span className="dv-c"/></div>;
  const cls = cell.k === "+" ? " add" : cell.k === "-" ? " del" : "";
  return <div className={`dv-l${cls}`}>
    <span className="dv-g" onClick={open && at ? () => open(...at) : undefined}><span>{cell.num}</span></span>
    <TextCell text={cell.text} eol={cell.eol}/>
  </div>;
}

function TextCell({ text, eol }: { text: string; eol: boolean }) {
  return <span className="dv-c">{text}{eol && <span className="dv-eol" title="No newline at end of file">no newline at end</span>}</span>;
}
