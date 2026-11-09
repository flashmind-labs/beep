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
  await agents.click();
  await expect(notes).toHaveAttribute("data-test-identity", "kept");
  await page.locator('.rs [data-act="room"]:visible').click();
  const room = tabs.getByRole("tab", { name: "Room Close Room" });
  await expect(room).toHaveAttribute("aria-selected", "true");
  await tabs.locator('[data-act="close:room"]').click();
  await expect(room).toHaveCount(0);
  await expect(agents).toHaveAttribute("aria-selected", "true");
});
