import test from "node:test";
import assert from "node:assert/strict";
import { cardLane, claim, filterCards, heldText, needKind, onlineCount, ownedIssues, teamView, teammates, type TeamRecord } from "../src/project/team-state.ts";

const record: TeamRecord = {
  repo: "github.com/org/web", me: "dana", pickup: true, pickup_limit: null, asks: true, removed: [],
  members: [
    { login: "kim", mesh: "b", joined_at: 2 },
    { login: "dana", mesh: "a", joined_at: 1 },
    { login: "lee", mesh: "c", joined_at: 3 },
  ],
};
const team = { kind: "team", record, me: "dana" } as const;

test("the board's filter keeps everyone's cards, the user's, or one teammate's", () => {
  const cards = [{ login: "dana", key: "own" }, { login: "Kim", key: "kim's" }, { login: "lee", key: "lee's" }];

  assert.deepEqual(filterCards(cards, { kind: "everyone" }, "dana").map(card => card.key), ["own", "kim's", "lee's"]);
  assert.deepEqual(filterCards(cards, { kind: "mine" }, "Dana").map(card => card.key), ["own"]);
  assert.deepEqual(filterCards(cards, { kind: "member", login: "kim" }, "dana").map(card => card.key), ["kim's"]);
});

test("an issue is free, the user's, or held by the others on it", () => {
  assert.deepEqual(claim({ assignees: [] }, "dana"), { kind: "free" });
  assert.deepEqual(claim({ assignees: ["kim", "Dana"] }, "dana"), { kind: "mine" });
  assert.deepEqual(claim({ assignees: ["kim", "lee"] }, "dana"), { kind: "held", by: ["kim", "lee"] });
});

test("a record without this mesh on it is no team", () => {
  assert.deepEqual(teamView(null), { kind: "none" });
  assert.deepEqual(teamView({ ...record, me: null }), { kind: "none" });
  assert.deepEqual(teamView(record), team);
});

test("queued cards and lanes a newer node names wait in Working", () => {
  assert.equal(cardLane("needs_you"), "needs");
  assert.equal(cardLane("queued"), "working");
  assert.equal(cardLane("working"), "working");
  assert.equal(cardLane("ready_to_review"), "review");
  assert.equal(cardLane("done_today"), "done");
  assert.equal(cardLane("blocked"), "working");
});

test("a Needs you card is an ask, an issue picked up, or unknown", () => {
  assert.deepEqual(needKind({ id: "ask:7f", issue: null }), { kind: "ask", id: "7f" });
  assert.deepEqual(needKind({ id: "pickup:52", issue: 52 }), { kind: "pickup", issue: 52 });
  assert.deepEqual(needKind({ id: "later:1", issue: null }), { kind: "other" });
});

test("the user and teammates with cards count as online, teammates in the order they joined", () => {
  assert.equal(onlineCount(team, []), 1);
  assert.equal(onlineCount(team, [{ login: "kim" }, { login: "kim" }, { login: "gone" }]), 2);
  assert.deepEqual(teammates(team), ["kim", "lee"]);
});

test("the Issues list shows all issues, the user's, or nobody's", () => {
  const issues = [{ n: 1, assignees: ["Dana"] }, { n: 2, assignees: [] }, { n: 3, assignees: ["kim"] }];

  assert.deepEqual(ownedIssues(issues, "all", "dana").map(issue => issue.n), [1, 2, 3]);
  assert.deepEqual(ownedIssues(issues, "mine", "dana").map(issue => issue.n), [1]);
  assert.deepEqual(ownedIssues(issues, "unassigned", "dana").map(issue => issue.n), [2]);
});

test("the held sheet names every holder", () => {
  assert.equal(heldText(["kim"], 52), "kim has #52");
  assert.equal(heldText(["kim", "lee"], 52), "kim and lee have #52");
  assert.equal(heldText(["kim", "lee", "max"], 52), "kim, lee and max have #52");
});
