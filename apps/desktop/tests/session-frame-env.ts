// A session frame as `drawAgentFrame` shows it, for tests that draw an
// agent's log and field. Import it after the DOM environment.
import { createElement } from "react";
import "../src/bar/bar-app.tsx";
import { SessionField, showSessionFrame } from "../src/agents/session-frame.tsx";
import { agentHints } from "../src/agents/session-head.tsx";
import type { ChatBody } from "../src/ui/turn.ts";

/// Shows an empty log body, 800px tall, and with `field` the message field.
export function showSessionBody(field = false) {
  const body = document.createElement("div") as ChatBody;
  body.className = "dbody";
  body.id = "agbody";
  Object.defineProperty(body, "clientHeight", { configurable: true, value: 800 });
  const fieldView = field ? createElement(SessionField, { mode: "writable", name: "Claude Code", banner: "", moveKey: "", machine: "build-box", session: "s1", continueHere: null, attach: () => {}, draft: "test", removeLast: () => {}, send: () => {}, stop: () => {}, openParent: () => {}, retry: null, cancel: () => {} }) : null;
  showSessionFrame({ head: null, field: fieldView, body, hints: agentHints() }, false, () => {});
  return body;
}
