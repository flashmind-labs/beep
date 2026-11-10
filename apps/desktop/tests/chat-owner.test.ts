import { bar } from "./chat-dom-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { appendTurn, createChatBody, createTurn, disposeChatBody, turnElement } from "../src/chat/transcript.ts";

test("retained chat bodies keep independent transcripts", () => {
  const first = createChatBody(), second = createChatBody();
  bar.append(first, second);
  const firstTurn = createTurn(first, { prompt: "First" }), secondTurn = createTurn(second, { prompt: "Second" });
  appendTurn(firstTurn);
  appendTurn(secondTurn);
  const firstElement = turnElement(firstTurn), secondElement = turnElement(secondTurn);
  assert.equal(first.querySelector(".turn"), firstElement);
  second.remove();
  disposeChatBody(first);
  assert.equal(first.querySelector(".turn"), null);
  bar.append(second);
  assert.equal(second.querySelector(".turn"), secondElement);
  assert.equal(secondElement?.textContent, "YouSecond");
  disposeChatBody(second);
});
