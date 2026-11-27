// A review comment in the diff: as saved, or in its editor. An editor that
// moves, as when its line leaves the diff, keeps its text, its selection and
// the focus.
import { useLayoutEffect, useRef } from "react";
import { useStore } from "zustand";
import { agentLabel } from "../../stores/project/projects-state.ts";
import { HarnessIcon } from "../../ui/icons.tsx";
import type { ReviewComment } from "../../stores/review/diff-state.ts";
import type { Viewer } from "../../stores/review/diff-viewer.ts";
import { cancelComment, openEditor, removeComment, saveComment } from "../../stores/review/diff-comments.ts";
import { Button } from "../../ui/kit.tsx";

export function DiffComment({ v, comment, label }: { v: Viewer; comment: ReviewComment; label: string }) {
  const editing = useStore(v.store, s => s.editors.has(comment));
  if (editing) return <Editor v={v} comment={comment} />;

  const by = comment.by ?? "", name = by ? agentLabel(by) : "";
  return <div className="dv-cmt">
    <div className="b">{by && <span className="by" title={`Written by ${name}`}>
      <HarnessIcon agent={by} />{name}
    </span>}{label && <><code>{label}</code> </>}{comment.text}</div>
    <div className="a"><button type="button" onClick={() => openEditor(v, comment, false)}>Edit</button>
      <button type="button" aria-label="Remove the comment" onClick={() => removeComment(v, comment)}>×</button></div>
  </div>;
}

function Editor({ v, comment }: { v: Viewer; comment: ReviewComment }) {
  const input = useRef<HTMLTextAreaElement>(null);
  const place = comment.line === null ? "Comment on this file" : "Comment on this line";

  // A fresh editor takes the focus; one that moved takes up where it was.
  useLayoutEffect(() => {
    const node = input.current;
    if (!node) return;
    const snapshot = v.snapshots.get(comment);
    v.snapshots.delete(comment);

    if (!snapshot) node.focus({ preventScroll: true });
    else {
      node.value = snapshot.text;
      if (snapshot.focused) { node.focus({ preventScroll: true }); node.scrollIntoView({ block: "nearest" }); }
      node.setSelectionRange(snapshot.start, snapshot.end);
    }

    return () => {
      if (!v.store.getState().editors.has(comment)) return;
      v.snapshots.set(comment, { text: node.value, start: node.selectionStart, end: node.selectionEnd, focused: document.activeElement === node });
    };
  }, [v, comment]);

  const save = () => {
    const text = input.current?.value.trim() ?? "";
    if (text) saveComment(v, comment, text);
  };

  return <div className="dv-cmt open">
    <textarea ref={input} rows={2} autoComplete="off" autoCorrect="off" defaultValue={comment.text} placeholder={place} aria-label={place}
      onKeyDown={event => {
        if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); cancelComment(v, comment); v.root.focus({ preventScroll: true }); }
        else if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) { event.preventDefault(); save(); }
      }}/>
    <div className="a"><Button look="primary" small onClick={save}>Add</Button>
      <Button look="quiet" small onClick={() => cancelComment(v, comment)}>Cancel</Button></div>
  </div>;
}
