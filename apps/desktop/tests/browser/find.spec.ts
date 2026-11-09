import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

for (const width of [1280, 640]) {
  test(`Find overlays the conversation without moving messages at ${width}px`, async ({ page }, info) => {
    await page.setViewportSize({ width, height: 900 });
    await openDesktop(page, true);
    await page.evaluate(() => window.__navigation.open({ agent: {
      kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex",
      title: "First notification", cwd: "/srv/project", status: "working", doing: null,
      started_at: 1, last_activity: 1, external: false,
    } }));
    const body = page.locator("#agbody");
    await expect(page.locator("#agtitle")).toHaveText("First notification");
    await page.locator("#agtitle").click();
    await body.evaluate(node => {
      for (let i = 0; i < 80; i++) {
        const message = document.createElement("p");
        message.textContent = `Example message ${i}`;
        node.append(message);
      }

      node.scrollTop = 400;
    });
    const metrics = () => body.evaluate(node => ({
      top: node.getBoundingClientRect().top, height: node.clientHeight, scroll: node.scrollTop,
    }));
    const before = await metrics();
    await page.keyboard.press(info.project.name === "Windows" ? "Control+f" : "Meta+f");
    const bar = page.getByRole("search", { name: "Search conversation" });
    await expect(bar.getByRole("textbox")).toBeFocused();
    expect(await metrics()).toEqual(before);
    expect(await bar.evaluate(node => {
      const rect = node.getBoundingClientRect();
      return node.contains(document.elementFromPoint(rect.x + rect.width / 2, rect.y + rect.height / 2));
    })).toBe(true);
    const barBox = await bar.boundingBox();
    const bodyBox = await body.boundingBox();
    expect(barBox).not.toBeNull();
    expect(bodyBox).not.toBeNull();

    if (barBox && bodyBox) {
      expect(barBox.y).toBeGreaterThanOrEqual(bodyBox.y);
      expect(barBox.y + barBox.height).toBeLessThan(bodyBox.y + bodyBox.height);
      expect(barBox.x).toBeGreaterThanOrEqual(bodyBox.x);
      expect(barBox.x + barBox.width).toBeLessThanOrEqual(bodyBox.x + bodyBox.width);
    }

    await page.keyboard.press("Escape");
    await expect(bar).toHaveCount(0);
    await expect(page.locator(".chat-find-layer")).toHaveCount(0);
    expect(await metrics()).toEqual(before);
  });
}

test("search keeps its field and draft while results and regex mode change", async ({ page }, info) => {
  const user = { role: "user", text: "Find the answer", at: 1, images: [], model: "", thinking: "" };
  const answer = { role: "assistant", html: "<p>First answer and second answer.</p>", tools: [], replies: [], model: "" };
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, open_session: [user, answer], session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.evaluate(() => window.__navigation.open({ chat: { session: "chat-example" } }));
  await expect(page.locator("#body")).toContainText("second answer");
  await page.keyboard.press(info.project.name === "Windows" ? "Control+f" : "Meta+f");
  const search = page.getByRole("search", { name: "Search conversation" }), input = search.getByRole("textbox");
  await input.fill("answer");
  await expect(search.locator("output")).toContainText("of 3");
  await input.evaluate(el => Reflect.set(window, "findInput", el));
  await search.getByRole("button", { name: "Use regular expression" }).click();
  await expect(input).toBeFocused();
  expect(await input.evaluate(el => el === Reflect.get(window, "findInput"))).toBe(true);
  await input.fill("[");
  await expect(search.locator("output")).toHaveText("Invalid regex");
  await expect(input).toHaveAttribute("aria-invalid", "true");
  await search.getByRole("button", { name: "Close search (Esc)" }).click();
  await expect(search).toHaveCount(0);
});
