import { useLayoutEffect, useRef, useState } from "react";
import { agentLabel } from "../project/projects-state.ts";
import { harnessInner, harnessMark } from "../ui/harnessicon.ts";
import type { ReviewComment } from "./diff-state.ts";

interface DiffCommentProps {
  comment: ReviewComment;
  label: string;
  initiallyEditing: boolean;
  isNew: boolean;
  onSave: (value: string) => void;
  onRemove: () => void;
  onCancelNew: () => void;
  onEscape: () => void;
}

export function DiffComment({ comment, label, initiallyEditing, isNew, onSave, onRemove, onCancelNew, onEscape }: DiffCommentProps) {
  const [editing, setEditing] = useState(initiallyEditing);
  const input = useRef<HTMLTextAreaElement>(null);
  useLayoutEffect(() => { if (editing) input.current?.focus({ preventScroll: true }); }, [editing]);
  const place = comment.line === null ? "Comment on this file" : "Comment on this line";
  const save = () => {
    const text = input.current?.value.trim() ?? "";
    if (!text) return;
    setEditing(false);
    onSave(text);
  };
  const cancel = () => { if (isNew) onCancelNew(); else setEditing(false); };

  if (editing) return <div className="dv-cmt open">
    <textarea ref={input} rows={2} defaultValue={comment.text} placeholder={place} aria-label={place}
      onKeyDown={event => {
        if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); cancel(); onEscape(); }
        else if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) { event.preventDefault(); save(); }
      }}/>
    <div className="a"><button type="button" className="dbtn small pri" onClick={save}>Add</button>
      <button type="button" className="dbtn small quiet" onClick={cancel}>Cancel</button></div>
  </div>;

  const by = comment.by ?? "", name = by ? agentLabel(by) : "", mark = by ? harnessMark(by) : null;
  const tile = `cc-ag${/^[a-z0-9-]+$/.test(by) ? ` ${by}` : ""}${mark ? " mark" : ""}`;
  return <div className="dv-cmt">
    <div className="b">{by && <span className="by" title={`Written by ${name}`}>
      <span className={tile} dangerouslySetInnerHTML={{ __html: harnessInner(by) }}/>{name}
    </span>}{label && <><code>{label}</code> </>}{comment.text}</div>
    <div className="a"><button type="button" onClick={() => setEditing(true)}>Edit</button>
      <button type="button" aria-label="Remove the comment" onClick={onRemove}>×</button></div>
  </div>;
}
