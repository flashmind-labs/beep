import { bar } from "./steer-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { disposeLog, fillLog, setUnits } from "../src/agents/logwindow.ts";
import type { ActivityRow } from "../src/agents/activity-view.tsx";
import type { ChatBody } from "../src/ui/turn.ts";

globalThis.getComputedStyle = () => ({ rowGap: "0" }) as CSSStyleDeclaration;

test("streaming thoughts keep their element during refresh", () => {
  bar.innerHTML = '<div id="agbody"></div>';
  const body = bar.querySelector<ChatBody>("#agbody"); assert.ok(body);
  Object.defineProperty(body, "clientHeight", { configurable: true, value: 800 });
  body.innerHTML = '<div class="dcoderlog"></div>';
  const log = body.querySelector<HTMLElement>(".dcoderlog"); assert.ok(log);
  const row = (text: string): ActivityRow => ({ kind: "thought", id: "1", key: "session:1", at: 1, text, content: null, ms: null, live: true, open: true, html: text });
  const units = (text: string) => [{ key: "turn", rows: () => [row(text)] }];
  setUnits(log, units("First thought"), []); fillLog(body, false);
  const thought = log.querySelector<HTMLElement>(".dthink[data-row]");
  const text = thought?.querySelector<HTMLElement>(".kk");
  assert.ok(thought && text);
  const selected = text.firstChild; assert.ok(selected);
  assert.equal(setUnits(log, units("First thought continues"), [selected]), false);
  assert.equal(text.textContent, "First thought");
  setUnits(log, units("First thought continues"), []);
  assert.equal(log.querySelector(".dthink[data-row]"), thought);
  assert.equal(thought.querySelector(".kk"), text);
  assert.match(text.textContent ?? "", /continues/);
  log.remove();
  body.append(log);
  assert.equal(log.querySelector(".dthink[data-row]"), thought);
  disposeLog(log);
  assert.equal(log.querySelector(".dthink[data-row]"), null);
});

