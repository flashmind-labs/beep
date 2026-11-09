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
