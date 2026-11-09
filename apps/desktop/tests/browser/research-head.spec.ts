import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const research = (topic: string) => ({ topic, status: "researching", agents: [], entries: [], sources: [], draft: "", draft_html: "", started_at: 1, finished_at: null, round: 1 });

test("research Stop reaches the current session after navigation rebuilds the header", async ({ page }) => {
  await openDesktop(page, false, {
    status: { link: "linked", signed_out: false, server: true, live: false, session: null },
    cached_research: research("First topic"), get_research: research("First topic"), research_history: [], research_pending: null,
    list_researches: [],
  });
  await page.evaluate(() => window.__navigation.open({ open_research: { session: "first" } }));
  const head = page.locator(".rs .dhead:visible");
  await expect(head).toContainText("First topic");
  await head.getByRole("button", { name: "Stop" }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "stop_research").map(call => call.args.session))).toContain("first");
  await page.evaluate(data => {
    window.__navigation.respond("cached_research", data);
    window.__navigation.respond("get_research", data);
    window.__navigation.open({ open_research: { session: "second" } });
  }, research("Second topic"));
  await expect(head).toContainText("Second topic");
  await head.getByRole("button", { name: "Stop" }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "stop_research").map(call => call.args.session))).toContain("second");
});

test("research tabs preserve their elements and delegated open and close actions", async ({ page }) => {
  await openDesktop(page, false, {
    status: { link: "linked", signed_out: false, server: true, live: false, session: null },
    cached_research: research("Example topic"), get_research: research("Example topic"), research_history: [], research_pending: null,
    list_researches: [],
  });
  await page.evaluate(() => window.__navigation.open({ open_research: { session: "first" } }));
  const tabs = page.locator(".rs .agtabs:visible");
  const agents = tabs.getByRole("tab", { name: "Agents" });
  const notes = tabs.getByRole("tab", { name: "Notes" });
  await expect(agents).toHaveAttribute("aria-selected", "true");
  await notes.evaluate(el => { el.setAttribute("data-test-identity", "kept"); });
  await notes.click();
  await expect(notes).toHaveAttribute("aria-selected", "true");
  await expect(page.locator(".rs .agpviews .rpane:visible")).toHaveCount(1);
  await agents.click();
  await expect(page.locator(".rs .agpviews .rpane:visible")).toHaveCount(1);
  await expect(notes).toHaveAttribute("data-test-identity", "kept");
  await page.locator('.rs [data-act="room"]:visible').click();
  const room = tabs.getByRole("tab", { name: "Room Close Room" });
  await expect(room).toHaveAttribute("aria-selected", "true");
  await tabs.locator('[data-act="close:room"]').click();
  await expect(room).toHaveCount(0);
  await expect(agents).toHaveAttribute("aria-selected", "true");
});

test("research sources keep scroll and links across polling updates", async ({ page }) => {
  const sources = Array.from({ length: 40 }, (_, i) => ({ number: i + 1, url: `https://example.test/source-${i + 1}`, title: `Source ${i + 1}`, quote: `Quote ${i + 1}`, agent: "scout-1" }));
  const initial = { ...research("Sources example"), sources };
  await openDesktop(page, false, {
    status: { link: "linked", signed_out: false, server: true, live: false, session: null },
    cached_research: initial, get_research: initial, research_history: [], research_pending: null,
    list_researches: [],
  });
  await page.evaluate(() => window.__navigation.open({ open_research: { session: "first" } }));
  const tabs = page.locator(".rs .agtabs:visible");
  await tabs.getByRole("tab", { name: /Sources/ }).click();
  const pane = page.locator(".rs .agpviews .rpane:visible");
  const link = pane.getByRole("link", { name: "Source 1", exact: true });
  await expect(link).toBeAttached();
  await link.evaluate(el => el.setAttribute("data-test-identity", "kept"));
  const before = await pane.evaluate(el => { el.scrollTop = el.scrollHeight / 2; return el.scrollTop; });
  expect(before).toBeGreaterThan(0);
  const updated = { ...initial, sources: [...sources, { number: 41, url: "https://example.test/source-41", title: "Source 41", quote: "Quote 41", agent: "scout-1" }] };
  await page.evaluate(data => window.__navigation.respond("get_research", data), updated);
  await page.locator(".rs .dhead:visible").getByRole("button", { name: "Stop" }).click();
  await expect(pane).toContainText("Source 41");
  await expect(pane).toHaveJSProperty("scrollTop", before);
  await expect(link).toHaveAttribute("data-test-identity", "kept");
  await link.click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "open_url").map(call => call.args.url))).toContain("https://example.test/source-1");
  await tabs.getByRole("tab", { name: "Agents" }).click();
  await tabs.getByRole("tab", { name: /Sources/ }).click();
  await expect(pane).toContainText("Source 41");
});

test("research notes and room preserve scroll and update incrementally", async ({ page }) => {
  const notes = Array.from({ length: 30 }, (_, i) => ({ seq: i + 1, agent: "scout-1", at: 1, kind: "note", content: `Note ${i + 1}`, sources: i === 0 ? [1, 2] : [] }));
  const messages = Array.from({ length: 30 }, (_, i) => ({ seq: i + 31, agent: "scout-1", at: 1, kind: "said", content: `Message ${i + 1}`, to: null }));
  const initial = { ...research("Notes and room"), entries: [...notes, ...messages], sources: [{ number: 1, url: "https://example.test/source", title: "Source title", quote: "Quote", agent: "scout-1" }] };
  await openDesktop(page, false, {
    status: { link: "linked", signed_out: false, server: true, live: false, session: null },
    cached_research: initial, get_research: initial, research_history: [], research_pending: null, list_researches: [],
  });
  await page.evaluate(() => window.__navigation.open({ open_research: { session: "first" } }));
  const tabs = page.locator(".rs .agtabs:visible");
  await tabs.getByRole("tab", { name: /Notes/ }).click();
  const pane = page.locator(".rs .agpviews .rpane:visible");
  const firstNote = pane.locator(".rmsg").first();
  await expect(firstNote).toContainText("Note 1");
  await expect(firstNote.locator("sup")).toHaveText("1, 2");
  const cite = firstNote.getByRole("link", { name: "1" });
  await cite.click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "open_url").map(call => call.args.url))).toContain("https://example.test/source");
  await firstNote.evaluate(el => el.setAttribute("data-test-identity", "kept"));
  const before = await pane.evaluate(el => { el.scrollTop = el.scrollHeight / 2; return el.scrollTop; });
  expect(before).toBeGreaterThan(0);
  const updated = { ...initial, entries: [...initial.entries, { seq: 61, agent: "scout-1", at: 1, kind: "note", content: "A new note", sources: [] }, { seq: 62, agent: "scout-1", at: 1, kind: "said", content: "A new message", to: null }] };
  await page.evaluate(data => window.__navigation.respond("get_research", data), updated);
  await page.locator(".rs .dhead:visible").getByRole("button", { name: "Stop" }).click();
  await expect(pane).toContainText("A new note");
  await expect(pane).toHaveJSProperty("scrollTop", before);
  await expect(firstNote).toHaveAttribute("data-test-identity", "kept");
  await tabs.getByRole("tab", { name: "Agents" }).click();
  await page.locator('.rs [data-act="room"]:visible').click();
  await expect(pane).toContainText("A new message");
  await expect.poll(() => pane.evaluate(el => el.scrollHeight - el.clientHeight - el.scrollTop)).toBeLessThanOrEqual(1);
  await pane.evaluate(el => { el.scrollTop = 0; });
  const later = { ...updated, entries: [...updated.entries, { seq: 63, agent: "scout-1", at: 1, kind: "said", content: "Another message", to: null }] };
  await page.evaluate(data => window.__navigation.respond("get_research", data), later);
  await page.locator(".rs .dhead:visible").getByRole("button", { name: "Stop" }).click();
  await expect(pane).toContainText("Another message");
  await expect(pane).toHaveJSProperty("scrollTop", 0);
});

test("research chat and paper keep their content and actions across updates", async ({ page }) => {
  const completed = { ...research("Paper example"), status: "done", draft: "Paper", draft_html: "<p>A finding [1].</p>", sources: [{ number: 1, url: "https://example.test/source", title: "Source title", quote: "Quote", agent: "scout-1" }] };
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, cached_research: completed, get_research: completed, research_history: [{ role: "user", text: "Question" }, { role: "assistant", html: "<p>Research complete.</p>" }], research_pending: null, list_researches: [] });
  await page.evaluate(() => window.__navigation.open({ open_research: { session: "first" } }));
  const chat = page.locator(".rs .dcoderlog:visible"), question = chat.locator(".dcoderen.user");
  await expect(question).toContainText("Question");
  await question.evaluate(el => Reflect.set(window, "researchQuestion", el));
  await expect(chat).toContainText("Research complete.");
  await chat.getByRole("button", { name: "Open the paper" }).click();
  const paper = page.locator(".rs .rdoc:visible");
  await expect(paper).toContainText("A finding");
  await paper.getByRole("link", { name: "1" }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "open_url").map(call => call.args.url))).toContain("https://example.test/source");
  expect(await question.evaluate(el => el === Reflect.get(window, "researchQuestion"))).toBe(true);
  await expect(page.getByRole("button", { name: "Copy", exact: true }).last()).toBeVisible();
});

test("an updated Research answer preserves selected unchanged text", async ({ page }) => {
  const first = research("Selection example");
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, cached_research: first, get_research: first, research_history: [{ role: "assistant", html: "<p>Keep this passage.</p>" }], research_pending: null, list_researches: [] });
  await page.evaluate(() => window.__navigation.open({ open_research: { session: "first" } }));
  const answer = page.locator(".rs .dcoderen.agent p").first();
  await expect(answer).toHaveText("Keep this passage.");
  await answer.evaluate(el => { const node = el.firstChild; if (!node) return; const range = document.createRange(); range.setStart(node, 0); range.setEnd(node, 4); const selection = window.getSelection(); selection?.removeAllRanges(); selection?.addRange(range); Reflect.set(window, "researchAnswer", el); });
  await page.evaluate(data => { window.__navigation.respond("research_history", [{ role: "assistant", html: "<p>Keep this passage.</p><p>A new paragraph.</p>" }]); window.__navigation.respond("get_research", data); }, { ...first, status: "done" });
  await page.locator(".rs .dhead:visible").getByRole("button", { name: "Stop" }).click();
  await expect(page.locator(".rs .dcoderen.agent")).toContainText("A new paragraph.");
  expect(await answer.evaluate(el => el === Reflect.get(window, "researchAnswer") && window.getSelection()?.toString() === "Keep")).toBe(true);
});

test("agent threads retain selected entries and scroll as new work arrives", async ({ page }) => {
  const agent = { id: "scout-1", name: "Scout", role: "Find sources", model: "Example", round: 1 };
  const entries = Array.from({ length: 30 }, (_, i) => ({ seq: i + 1, agent: "scout-1", at: 1, kind: "note", content: `Finding ${i + 1}`, sources: [] }));
  const initial = { ...research("Thread example"), agents: [agent], entries };
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, cached_research: initial, get_research: initial, research_history: [], research_pending: null, list_researches: [] });
  await page.evaluate(() => window.__navigation.open({ open_research: { session: "first" } }));
  await page.locator('.rs [data-act="agent:scout-1"]').click();
  const pane = page.locator(".rs .rpane.thread:visible"), message = pane.locator(".rmsg").first();
  await expect(message).toContainText("Finding 1");
  await message.evaluate(el => { const node = el.querySelector("p")?.firstChild; if (!node) return; const range = document.createRange(); range.setStart(node, 0); range.setEnd(node, 7); window.getSelection()?.removeAllRanges(); window.getSelection()?.addRange(range); Reflect.set(window, "researchThreadMessage", el); });
  const scroll = await pane.evaluate(el => { el.scrollTop = el.scrollHeight / 2; return el.scrollTop; });
  await page.evaluate(data => window.__navigation.respond("get_research", data), { ...initial, entries: [...entries, { seq: 31, agent: "scout-1", at: 1, kind: "note", content: "New finding", sources: [] }] });
  await page.locator(".rs .dhead:visible").getByRole("button", { name: "Stop" }).click();
  await expect(pane).toContainText("New finding");
  await expect(pane).toHaveJSProperty("scrollTop", scroll);
  expect(await message.evaluate(el => el === Reflect.get(window, "researchThreadMessage") && window.getSelection()?.toString() === "Finding")).toBe(true);
});

test("Research updates retain the composer node, draft and selection", async ({ page }) => {
  const initial = research("Draft example");
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, cached_research: initial, get_research: initial, research_history: [], research_pending: null, list_researches: [] });
  await page.evaluate(() => window.__navigation.open({ open_research: { session: "first" } }));
  const input = page.getByRole("textbox", { name: "Message the lead" });
  await input.fill("A draft message");
  await input.evaluate(el => { if (el instanceof HTMLTextAreaElement) { el.setSelectionRange(2, 7); Reflect.set(window, "researchDraft", el); } });
  await page.evaluate(data => window.__navigation.respond("get_research", data), { ...initial, topic: "Updated example" });
  await page.locator(".rs .dhead:visible").getByRole("button", { name: "Stop" }).evaluate(el => { if (el instanceof HTMLElement) el.click(); });
  await expect(page.locator(".rs .dhead:visible")).toContainText("Updated example");
  expect(await input.evaluate(el => el === Reflect.get(window, "researchDraft") && document.activeElement === el && el instanceof HTMLTextAreaElement && el.value === "A draft message" && el.selectionStart === 2 && el.selectionEnd === 7)).toBe(true);
});
