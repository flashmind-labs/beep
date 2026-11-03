import test from "node:test";
import assert from "node:assert/strict";
import { newChatProject } from "../src/project.ts";

test("new chat goes to the project on screen, else a general chat", () => {
  const agents = [{ name: "acme/widget", chat_id: "c1" }];
  assert.equal(newChatProject({ shown: "projects", picked: "acme/widget" }, agents), "acme/widget");
  assert.equal(newChatProject({ shown: "projects", picked: null }, agents), null);
  assert.equal(newChatProject({ shown: "pane", view: "chat", session: "c1" }, agents), "acme/widget");
  assert.equal(newChatProject({ shown: "pane", view: "chat", session: "c9" }, agents), null);
  assert.equal(newChatProject({ shown: "pane", view: "chat", session: null }, agents), null);
  assert.equal(newChatProject({ shown: "pane", view: "memories", session: "c1" }, agents), null);
  assert.equal(newChatProject({ shown: "home", picked: "acme/widget" }, agents), null);
});
