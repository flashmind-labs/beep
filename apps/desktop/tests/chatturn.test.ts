import { answers, bar, removed } from "./dom-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { body, follow, newTurn, startRun } from "../src/bar/chat.ts";
import type { BarEvent } from "../src/bar/chat.ts";
import { S } from "../src/bar/core.ts";
import { refreshSession } from "../src/bar/history.ts";
import type { HistoryTurn } from "../src/bar/history.ts";

const SESSION = "00000000-0000-4000-8000-000000000001";
const PROMPT = "Will it rain tomorrow?";
const prompt: HistoryTurn = { role: "user", text: PROMPT, at: 1, images: [], model: "", thinking: "" };
const answered: HistoryTurn = {
  role: "assistant",
  html: "<p>No rain tomorrow, 21 degrees.</p>",
  tools: [{ name: "show_cards", label: "Showing the forecast", success: true, route: null, file: null, images: [], products: null, cards: '<div class="dprods"><div class="dprod">Forecast</div></div>', chart: null }],
  replies: ["And the weekend?", "What about Paris?"],
  model: "",
};

test("a reply stays the same element from its stream through the chat's redraws", async t => {
  t.mock.timers.enable({ apis: ["setTimeout", "setInterval"] });
  for (const cmd of ["session_agents", "session_files", "session_sources", "list_questions"]) answers[cmd] = [];
  S.view = "chat";
  S.session = SESSION;
  bar.innerHTML = '<div class="dbody" id="body"></div><div class="dhints"></div>';

  const turn = newTurn(PROMPT);
  turn.user = true;
  startRun(SESSION, turn, null);
  const events = follow(SESSION, turn);
  const send = (ev: BarEvent) => events.onmessage(ev);
  send({ kind: "started", run_id: "run-1", session_id: SESSION });
  send({ kind: "text", html: "<p>No rain</p>" });
  send({ kind: "text", html: "<p>No rain tomorrow.</p>" });
  const reply = turn.querySelector(".reply")!;
  send({ kind: "finished", status: "completed" });
  assert.equal(reply.innerHTML, "<p>No rain tomorrow.</p>");

  // The first notice comes before this Mac's copy has the reply.
  answers.open_session = [prompt];
  await refreshSession(SESSION);
  assert.ok(reply.isConnected, "the reply left the chat while the copy caught up");
  assert.equal(reply.innerHTML, "<p>No rain tomorrow.</p>");

  // Then the copy has it, with its follow-ups and cards.
  answers.open_session = [prompt, answered];
  await refreshSession(SESSION);
  const turns = body()!.querySelectorAll(".turn");
  assert.equal(turns.length, 1);
  assert.equal(turns[0], turn, "the turn was drawn again");
  assert.equal(turn.querySelector(".reply"), reply, "the reply was drawn again");
  assert.equal(reply.innerHTML, "<p>No rain tomorrow, 21 degrees.</p>");
  assert.deepEqual([...turn.querySelectorAll(".dreplies button")].map(b => b.textContent), ["And the weekend?", "What about Paris?"]);
  assert.ok(turn.querySelector(".dprod"), "the cards were not added");
  assert.ok(!removed.has(turn) && !removed.has(reply), "the reply was taken out of the page");
  assert.ok(!turn.querySelector(".ask") || !removed.has(turn.querySelector(".ask")!), "the prompt was taken out of the page");
});

test("a chat drawn from the server keeps its elements when it is drawn again", async () => {
  for (const cmd of ["session_agents", "session_files", "session_sources", "list_questions"]) answers[cmd] = [];
  S.view = "chat";
  S.session = SESSION;
  bar.innerHTML = '<div class="dbody" id="body"></div><div class="dhints"></div>';
  const later: HistoryTurn = { ...prompt, text: "And the weekend?", at: 2 };
  answers.open_session = [prompt, answered, later];
  await refreshSession(SESSION);
  const turns = [...body()!.querySelectorAll(".turn")];
  assert.deepEqual(turns.map(t => t.querySelector(".ask")?.textContent), [PROMPT, "And the weekend?"]);
  const reply = turns[0].querySelector(".reply")!;

  // The second prompt was rerun away.
  answers.open_session = [prompt, answered];
  await refreshSession(SESSION);
  assert.deepEqual([...body()!.querySelectorAll(".turn")], [turns[0]]);
  assert.equal(turns[0].querySelector(".reply"), reply);
  assert.ok(!removed.has(reply));
});
