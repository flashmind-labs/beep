// Where a diff's review comments wait to be sent: one entry per diff id.
import { type ReviewComment, parseDraft } from "./diff-state.ts";

export function loadDraft(id: string) {
  try { return parseDraft(localStorage.getItem(`beep.diffReview.${id}`)); } catch { return []; }
}

export function saveDraft(id: string, draft: readonly ReviewComment[]) {
  try {
    if (draft.length) localStorage.setItem(`beep.diffReview.${id}`, JSON.stringify(draft));
    else localStorage.removeItem(`beep.diffReview.${id}`);
  } catch { /* kept for this launch only */ }
}
