import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

test("mobile navigation retains tab buttons and places Back before the heading", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await openDesktop(page);
  await expect(page.locator("body")).toHaveClass(/mobile/);
  const tabs = page.getByRole("navigation", { name: "Sections" });
  const home = tabs.getByRole("button", { name: "Home", exact: true });
  await expect(home).toHaveAttribute("aria-current", "true");
  await home.evaluate(element => Reflect.set(window, "mobileHome", element));
  const projects = tabs.getByRole("button", { name: "Projects", exact: true });
  await projects.click();
  await expect(projects).toHaveAttribute("aria-current", "true");
  await page.evaluate(() => window.__navigation.open("instructions"));
  const back = page.locator(".mb-back");
  await expect(back).toBeVisible();
  expect(await back.evaluate(element => element.nextElementSibling?.id)).toBe("title");
  await back.click();
  await expect(projects).toHaveAttribute("aria-current", "true");
  await page.setViewportSize({ width: 1280, height: 900 });
  await expect(tabs).toBeHidden();
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(tabs).toBeVisible();
  expect(await home.evaluate(element => element === Reflect.get(window, "mobileHome"))).toBe(true);
});

test("mobile keyboard dismissal preserves the composer and its draft", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, open_session: [], session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.evaluate(() => window.__navigation.open({ chat: { session: "mobile-chat" } }));
  const input = page.locator("#ask");
  await input.fill("Draft to keep");
  const hide = page.getByRole("button", { name: "Hide the keyboard" });
  await expect(hide).toBeVisible();
  await input.evaluate(element => Reflect.set(window, "mobileDraft", element));
  await hide.click();
  await expect(input).not.toBeFocused();
  await expect(input).toHaveValue("Draft to keep");
  expect(await input.evaluate(element => element === Reflect.get(window, "mobileDraft"))).toBe(true);
  await input.focus();
  await expect(hide).toBeVisible();
  await page.evaluate(() => window.__navigation.open("home"));
  await expect(hide).toBeHidden();
});
