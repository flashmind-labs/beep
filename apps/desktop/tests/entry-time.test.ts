import "./dom-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { AgentPrompt, AgentReply } from "../src/agents/reply-view.tsx";

test("prompts and replies show their recorded time without changing message text", () => {
  const sentAt = 1794579242;
  for (const html of [
    renderToStaticMarkup(createElement(AgentPrompt, { prompt: { kind: "prompt", row: "1", html: "<p>Hello</p>", pictures: "", sentAt } })),
    renderToStaticMarkup(createElement(AgentReply, { reply: { kind: "reply", row: "2", markdown: true, html: "<p>Hello</p>", sentAt } })),
  ]) {
    assert.ok(html.includes(new Date(sentAt * 1000).toISOString()));
    assert.match(html, /<time[^>]+title=/);
    assert.ok(html.includes("<p>Hello</p>"));
  }
  const unknown = renderToStaticMarkup(createElement(AgentReply, { reply: { kind: "reply", row: "3", markdown: false, html: "Hello", sentAt: null } }));
  assert.ok(!unknown.includes("<time"));
});
