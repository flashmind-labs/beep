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
