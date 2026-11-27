import test from "node:test";
import assert from "node:assert/strict";
import { coverage, foundWhere, placeText, problemLines, placePicks, projectLines, skillPlace, toggleSkill, urlText, whereChip, type SkillLibrary } from "../src/stores/settings/skills-state.ts";

const skills = [{ name: "crawl", description: "Crawl a site" }, { name: "map", description: "List URLs" }, { name: "scrape", description: "Scrape a page" }];
const library = (part: Partial<SkillLibrary> = {}): SkillLibrary => ({ name: "firecrawl", url: "https://git.example.com/tools/firecrawl.git", revision: "a3f91c2d00", skills, global: "None", projects: {}, chats: {}, ...part });

test("a pick covers all, some or none of a library", () => {
  assert.equal(coverage("All", skills), "all");
  assert.equal(coverage({ Only: ["crawl", "map", "scrape"] }, skills), "all");
  assert.equal(coverage({ Only: ["map"] }, skills), "some");
  assert.equal(coverage("None", skills), "none");
  assert.equal(coverage({ Only: [] }, skills), "none");
});

test("turning one skill off an All pick keeps the others", () => {
  assert.deepEqual(toggleSkill("All", "map", false, skills), { Only: ["crawl", "scrape"] });
  assert.deepEqual(toggleSkill({ Only: ["map"] }, "map", false, skills), "None");
  assert.deepEqual(toggleSkill("None", "scrape", true, skills), { Only: ["scrape"] });
  assert.deepEqual(toggleSkill({ Only: ["scrape"] }, "crawl", true, skills), { Only: ["crawl", "scrape"] });
});

test("each skill says where it is installed", () => {
  const lib = library({ global: { Only: ["crawl"] }, projects: { "acme/widget": "All", "acme/docs": { Only: ["map"] }, "acme/game": "None" } });
  assert.equal(placeText(skillPlace(lib, "crawl")), "Global");
  assert.equal(placeText(skillPlace(lib, "map")), "acme/docs, acme/widget");
  assert.equal(placeText(skillPlace(library(), "map")), "Not installed");
});

test("the library chips name where and what it holds", () => {
  assert.deepEqual(whereChip(library({ global: "All" })), { text: "Global on every machine", title: "In each agent's skills folder" });
  assert.deepEqual(whereChip(library({ projects: { "acme/widget": "All", "acme/docs": { Only: ["map"] } } })), { text: "In 2 projects", title: "acme/docs, acme/widget" });
  assert.deepEqual(whereChip(library()), { text: "Not installed", title: null });
  assert.deepEqual(whereChip(library({ chats: { "chat-one": "All", "chat-two": "None" } })), { text: "In 1 chat", title: null });
  assert.deepEqual(whereChip(library({ projects: { "acme/widget": "All" }, chats: { "chat-one": "All" } })), { text: "In 1 project and 1 chat", title: "acme/widget" });
  assert.deepEqual(placePicks([library({ chats: { "chat-one": { Only: ["map"] } } })], { kind: "chat", chat: "chat-one" }), { firecrawl: { Only: ["map"] } });
  assert.equal(urlText(library().url), "git.example.com/tools/firecrawl");
});

test("the project sheet starts from the project's picks", () => {
  const writing = library({ name: "writing", global: "All" });
  const fc = library({ projects: { "acme/widget": { Only: ["map"] } } });
  const picks = placePicks([fc, writing], { kind: "project", project: "acme/widget" });
  assert.deepEqual(picks, { firecrawl: { Only: ["map"] }, writing: "None" });
  const [first, second] = projectLines([fc, writing], picks);
  assert.equal(first?.kind === "pick" && first.coverage, "some");
  assert.equal(first?.kind === "pick" && first.sub, "1 of 3 skills");
  assert.equal(second?.kind, "global");
});

test("found libraries and problems read as one line", () => {
  assert.equal(foundWhere({ url: "u", name: "ui", skills: ["a"], places: [{ machine: "build-box", folder: "~/.claude/skills" }] }), "~/.claude/skills on build-box");
  assert.equal(foundWhere({ url: "u", name: "ui", skills: ["a"], places: [{ machine: "a", folder: "x" }, { machine: "b", folder: "y" }] }), "2 skills folders");
  assert.deepEqual(problemLines([
    { machine: "build-box", library: "firecrawl", skill: "map", place: "acme/widget", message: "a skill there has this name" },
    { machine: "win-box", library: "firecrawl", skill: null, place: "global", message: "git clone failed" },
    { machine: "win-box", library: "other", skill: null, place: "global", message: "no" },
  ], "firecrawl"), ["build-box: map in acme/widget: a skill there has this name", "win-box: git clone failed"]);
});
