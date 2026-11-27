// The project page's pane for one pull request or issue: its state, labels, description and
// comments, drawn from threaddetail-state.ts.
import { useReducer, type ComponentProps } from "react";
import { ThreadDetailView, type ThreadActions } from "./threaddetail-view.tsx";
import type { ThreadDetails, ThreadStub } from "./threaddetail-state.ts";

/// The pane of one thread. `open` opens a link of it.
export function threadPane(details: ThreadDetails, projectId: string, stub: ThreadStub, acts: ThreadActions, open: (url: string) => void) {
  const { key, writing, edit, ...thread } = details.thread(projectId, stub);
  const view = (redraw: () => void): ComponentProps<typeof ThreadDetailView> => ({
    ...thread, ...writing(), acts, edit: (value: string) => { edit(value); redraw(); }, open,
  });

  return <div style={{ display: "contents" }} key={key}><ThreadPane view={view} /></div>;
}

function ThreadPane({ view }: { view: (redraw: () => void) => ComponentProps<typeof ThreadDetailView> }) {
  const [, redraw] = useReducer((n: number) => n + 1, 0);
  return <ThreadDetailView {...view(redraw)} />;
}
