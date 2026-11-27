// Which plan step a subagent ran under: the one running when its call began.

/// One step of a session's plan, and when it ran in Unix seconds as the
/// session's machine dated it; null when it never ran or nobody knows.
export interface PlanStep { content: string; status: string; started_at: number | null; ended_at: number | null }

/// The place of the step running at `at`, the last started when several
/// ran then; null when none did.
export function stepAt(plan: readonly PlanStep[], at: number): number | null {
  let found: number | null = null, latest = -Infinity;

  for (const [index, step] of plan.entries()) {
    const start = step.started_at;
    if (start == null || start > at || start < latest || (step.ended_at != null && at >= step.ended_at)) continue;

    found = index;
    latest = start;
  }

  return found;
}
