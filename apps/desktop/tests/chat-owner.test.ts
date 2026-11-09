import { bar } from "./chat-dom-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { chatBodyHost, createChatBody, createChatTurn, disposeChatBody } from "../src/chat/work-view.tsx";

test("retained chat scopes keep independent bodies and turns", () => {
  const first = createChatBody(), second = createChatBody();
  bar.append(chatBodyHost(first), chatBodyHost(second));
  const firstTurn = createChatTurn(first, { prompt: true, pictures: false });
  const secondTurn = createChatTurn(second, { prompt: true, pictures: false });
  const firstHost = firstTurn.parentElement, secondHost = secondTurn.parentElement;
  assert.ok(firstHost && secondHost);
  first.append(firstHost);
  second.append(secondHost);
  chatBodyHost(second).remove();
  disposeChatBody(first);
  assert.equal(firstTurn.isConnected, false);
  bar.append(chatBodyHost(second));
  assert.equal(secondTurn.isConnected, true);
  assert.equal(second.querySelector(".turn"), secondTurn);
  disposeChatBody(second);
});
