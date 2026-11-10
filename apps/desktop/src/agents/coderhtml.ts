// The identity a coding agent's question keeps between a chat and the
// session page, so an answer reaches only the question it was shown for.
import type { AgentQuestion, OpenAgent } from "./agents.ts";

/// Binds an approval action to the question and options shown to the user.
export const questionKey = (q: AgentQuestion, session: OpenAgent) => JSON.stringify([session.machine_id, session.agent, session.session, q.id, q.text, q.detail, q.options.map(o => [o.id, o.name, o.allow])]);

