import { useLayoutEffect, useRef, useState } from "react";
import { agentLabel } from "../project/projects-state.ts";
import { HarnessIcon } from "../ui/icons.tsx";
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
  onEditEnd: () => void;
  restored: { text: string; focused: boolean; start: number; end: number } | null;
}

export function DiffComment({ comment, label, initiallyEditing, isNew, onSave, onRemove, onCancelNew, onEscape, onEditEnd, restored }: DiffCommentProps) {
  const [editing, setEditing] = useState(initiallyEditing);
  const input = useRef<HTMLTextAreaElement>(null);
  useLayoutEffect(() => {
    if (!editing || !input.current || (restored && !restored.focused)) return;
    input.current.focus({ preventScroll: true });
    if (restored) input.current.setSelectionRange(restored.start, restored.end);
  }, [editing]);
  const place = comment.line === null ? "Comment on this file" : "Comment on this line";
  const save = () => {
    const text = input.current?.value.trim() ?? "";
    if (!text) return;
    setEditing(false);
    onSave(text);
    onEditEnd();
  };
  const cancel = () => { if (isNew) onCancelNew(); else setEditing(false); onEditEnd(); };

  if (editing) return <div className="dv-cmt open">
    <textarea ref={input} rows={2} defaultValue={restored?.text ?? comment.text} placeholder={place} aria-label={place}
      onKeyDown={event => {
        if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); cancel(); onEscape(); }
        else if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) { event.preventDefault(); save(); }
      }}/>
    <div className="a"><button type="button" className="dbtn small pri" onClick={save}>Add</button>
      <button type="button" className="dbtn small quiet" onClick={cancel}>Cancel</button></div>
  </div>;

  const by = comment.by ?? "", name = by ? agentLabel(by) : "";
  return <div className="dv-cmt">
    <div className="b">{by && <span className="by" title={`Written by ${name}`}>
      <HarnessIcon agent={by} />{name}
    </span>}{label && <><code>{label}</code> </>}{comment.text}</div>
    <div className="a"><button type="button" onClick={() => setEditing(true)}>Edit</button>
      <button type="button" aria-label="Remove the comment" onClick={onRemove}>×</button></div>
  </div>;
}
