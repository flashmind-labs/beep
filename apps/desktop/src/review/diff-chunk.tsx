import { Fragment, memo, useLayoutEffect, useRef } from "react";
import { lineKey, type DiffRow, type ReviewComment, type SplitCell, type SplitRow } from "./diff-state.ts";

type Row = DiffRow | SplitRow;
interface ChunkProps {
  rows: readonly Row[];
  comments: ReadonlyMap<string, readonly ReviewComment[]>;
  mountComment: (host: HTMLElement, comment: ReviewComment) => void;
  unmountComment: (host: HTMLElement) => void;
}

export const DiffChunk = memo(function DiffChunk({ rows, comments, mountComment, unmountComment }: ChunkProps) {
  return <>{rows.map((row, index) => {
    if (row.kind === "hunk") return <div key={index} className="dv-l hunk"><span className="dv-g"><span/><span/></span><span className="dv-c">{row.text}</span></div>;
    if (row.kind === "gap") return <div key={index} className="dv-l gap"><span className="dv-g"><span/><span/></span><span className="dv-c">{row.lines.toLocaleString("en-US")} unchanged {row.lines === 1 ? "line" : "lines"}</span></div>;
    if (row.kind === "pair") {
      const left = row.left?.k === "-" ? lineKey("old", row.left.num) : null;
      const right = row.right ? lineKey("new", row.right.num) : null;
      return <Fragment key={index}><div className="dv-pair"><Cell cell={row.left}/><Cell cell={row.right}/></div>
        <SavedComments keys={[left, right]} comments={comments} mountComment={mountComment} unmountComment={unmountComment}/></Fragment>;
    }
    const key = row.k === "-" && row.old !== null ? lineKey("old", row.old) : row.new !== null ? lineKey("new", row.new) : null;
    const cls = row.k === "+" ? " add" : row.k === "-" ? " del" : "";
    return <Fragment key={index}><div className={`dv-l${cls}`}><span className="dv-g"><span>{row.old ?? ""}</span><span>{row.new ?? ""}</span></span><TextCell text={row.text} eol={row.eol}/></div>
      <SavedComments keys={[key]} comments={comments} mountComment={mountComment} unmountComment={unmountComment}/></Fragment>;
  })}</>;
});

function Cell({ cell }: { cell: SplitCell | null }) {
  if (!cell) return <div className="dv-l none"><span className="dv-g"><span/></span><span className="dv-c"/></div>;
  const cls = cell.k === "+" ? " add" : cell.k === "-" ? " del" : "";
  return <div className={`dv-l${cls}`}><span className="dv-g"><span>{cell.num}</span></span><TextCell text={cell.text} eol={cell.eol}/></div>;
}

function TextCell({ text, eol }: { text: string; eol: boolean }) {
  return <span className="dv-c">{text}{eol && <span className="dv-eol" title="No newline at end of file">no newline at end</span>}</span>;
}

function SavedComments({ keys, comments, mountComment, unmountComment }: { keys: (string | null)[]; comments: ChunkProps["comments"]; mountComment: ChunkProps["mountComment"]; unmountComment: ChunkProps["unmountComment"] }) {
  return <>{keys.flatMap(key => key ? (comments.get(key) ?? []).map((comment, index) =>
    <CommentSlot key={`${key}:${index}`} comment={comment} mountComment={mountComment} unmountComment={unmountComment}/>) : [])}</>;
}

function CommentSlot({ comment, mountComment, unmountComment }: { comment: ReviewComment; mountComment: ChunkProps["mountComment"]; unmountComment: ChunkProps["unmountComment"] }) {
  const slot = useRef<HTMLSpanElement>(null);
  useLayoutEffect(() => {
    const host = slot.current;
    if (!host) return;
    mountComment(host, comment);
    return () => unmountComment(host);
  }, [comment, mountComment, unmountComment]);
  return <span ref={slot} className="dv-comment-slot" style={{ display: "contents" }}/>;
}
