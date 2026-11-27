// A chunk's rows, unified or side by side, coloured by the file's language
// with the places the filter finds marked, and the comments on each line
// under it. A click on a line's number opens a new comment there.
import { Fragment, type ReactNode } from "react";
import type { Span } from "../../ui/syntax.ts";
import { DiffGapControls } from "./diff-body-controls.tsx";
import { lineKey, occurrences, type ContextEdit, type DiffRow, type Hit, type ReviewComment, type SplitCell, type SplitRow } from "../../stores/review/diff-state.ts";

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
  look: Look;
}

/// How a chunk's text looks: the file's spans by line, the filter in lower
/// case, and the place stepped to when it is in this file.
export interface Look { spans: ReadonlyMap<string, readonly Span[]>; want: string; hit: Hit | null }

export function DiffRows({ rows, comments, readOnly, gutter, edit, comment, look }: RowsProps) {
  const open = readOnly ? null : gutter;
  const under = (keys: (string | null)[]) => keys.flatMap(key => key ? (comments.get(key) ?? []).map(comment) : []);

  return <>{rows.map((row, index) => {
    if (row.kind === "hunk") return <div key={index} className="dv-l hunk"><DiffGapControls row={row} edit={edit}/><span className="dv-c">{row.text}</span></div>;

    if (row.kind === "pair") {
      const { left, right } = row;
      // On an unchanged line the left cell takes its comment on the new side.
      const leftAt: [Side, number] | null = left?.k === "-" ? ["old", left.num] : left && right ? ["new", right.num] : null;
      return <Fragment key={index}>
        <div className="dv-pair"><Cell cell={left} side="old" at={leftAt} open={open} look={look}/><Cell cell={right} side="new" at={right ? ["new", right.num] : null} open={open} look={look}/></div>
        {under([left?.k === "-" ? lineKey("old", left.num) : null, right ? lineKey("new", right.num) : null])}
      </Fragment>;
    }

    const at: [Side, number] | null = row.k === "-" ? (row.old === null ? null : ["old", row.old]) : row.new === null ? null : ["new", row.new];
    const cls = row.k === "+" ? " add" : row.k === "-" ? " del" : "";
    return <Fragment key={index}>
      <div className={`dv-l${cls}`} data-at={at ? lineKey(...at) : undefined}>
        <span className="dv-g" onClick={open && at ? () => open(...at) : undefined}><span>{row.old ?? ""}</span><span>{row.new ?? ""}</span></span>
        <TextCell text={row.text} eol={row.eol} at={at} look={look}/>
      </div>
      {under([at ? lineKey(...at) : null])}
    </Fragment>;
  })}</>;
}

/// One side of a side by side row: `side` is the one its number is of, and
/// `at` where a comment on it goes.
function Cell({ cell, side, at, open, look }: { cell: SplitCell | null; side: Side; at: [Side, number] | null; open: Gutter | null; look: Look }) {
  if (!cell) return <div className="dv-l none"><span className="dv-g"><span/></span><span className="dv-c"/></div>;
  const cls = cell.k === "+" ? " add" : cell.k === "-" ? " del" : "";
  return <div className={`dv-l${cls}`} data-at={at ? lineKey(...at) : undefined}>
    <span className="dv-g" onClick={open && at ? () => open(...at) : undefined}><span>{cell.num}</span></span>
    <TextCell text={cell.text} eol={cell.eol} at={[side, cell.num]} look={look}/>
  </div>;
}

/// A line's text: `at` names its side and number, which its colours and a
/// current mark are kept by.
function TextCell({ text, eol, at, look }: { text: string; eol: boolean; at: [Side, number] | null; look: Look }) {
  const spans = at ? look.spans.get(lineKey(...at)) ?? NO_SPANS : NO_SPANS;
  const current = at && look.hit?.side === at[0] && look.hit.line === at[1] ? look.hit.at : null;
  return <span className="dv-c"><Code text={text} spans={spans} want={look.want} current={current}/>{eol && <span className="dv-eol" title="No newline at end of file">no newline at end</span>}</span>;
}

const NO_SPANS: readonly Span[] = [];

/// `text` cut where a colour or a mark starts or ends, each piece in its colour
/// and inside its mark.
function Code({ text, spans, want, current }: { text: string; spans: readonly Span[]; want: string; current: number | null }) {
  const marks = occurrences(text, want);
  if (!spans.length && !marks.length) return text;
  const cuts = new Set([0, text.length]);
  for (const span of spans) cuts.add(span.from).add(span.to);
  for (const mark of marks) cuts.add(mark).add(mark + want.length);
  const points = [...cuts].sort((a, b) => a - b), pieces: ReactNode[] = [];

  for (let i = 0; i + 1 < points.length; i++) {
    const from = points[i], to = points[i + 1], piece = text.slice(from, to);
    const span = spans.find(span => span.from <= from && to <= span.to);
    const mark = marks.find(mark => mark <= from && to <= mark + want.length);
    const coloured = span ? <span className={span.cls}>{piece}</span> : piece;
    pieces.push(<Fragment key={from}>{mark === undefined ? coloured : <mark className={mark === current ? "cur" : undefined}>{coloured}</mark>}</Fragment>);
  }

  return pieces;
}
