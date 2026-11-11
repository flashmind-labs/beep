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

test("on a phone, sheets and menus opened outside the main area lose shortcut hints", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await openDesktop(page);
  await expect(page.locator("body")).toHaveClass(/mobile/);
  // A sheet as the window mounts one, its content drawn after it opens.
  await page.evaluate(() => {
    const sheet = document.createElement("div");
    sheet.id = "test-sheet";
    sheet.innerHTML = '<button id="test-now" title="Close (esc)">Close</button>';
    document.body.append(sheet);
    requestAnimationFrame(() => { sheet.insertAdjacentHTML("beforeend", '<div><button id="test-later" title="Send (⌘⏎)">Send</button></div>'); });
  });
  await expect(page.locator("#test-now")).toHaveAttribute("title", "Close");
  await expect(page.locator("#test-later")).toHaveAttribute("title", "Send");
});

test("a swipe right from the left edge goes back on the phone", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await openDesktop(page);
  const tabs = page.getByRole("navigation", { name: "Sections" });
  const projects = tabs.getByRole("button", { name: "Projects", exact: true });
  await projects.click();
  await page.evaluate(() => window.__navigation.open("instructions"));
  await expect(page.locator(".mb-back")).toBeVisible();
  const swipe = (fromX: number, toX: number, dy = 0) => page.evaluate(([fromX, toX, dy]) => {
    const touch = (x: number, y: number) => new Touch({ identifier: 1, target: document.body, clientX: x, clientY: y });
    const fire = (type: string, x: number, y: number) => document.body.dispatchEvent(new TouchEvent(type, { bubbles: true, touches: type === "touchend" ? [] : [touch(x, y)], changedTouches: [touch(x, y)] }));
    fire("touchstart", fromX, 400);
    for (let i = 1; i <= 5; i++) fire("touchmove", fromX + (toX - fromX) * i / 5, 400 + dy * i / 5);
    fire("touchend", toX, 400 + dy);
  }, [fromX, toX, dy] as const);

  // Too short, or from the middle of the screen: nothing happens.
  await swipe(8, 50);
  await swipe(200, 360);
  await expect(page.locator(".mb-back")).toBeVisible();
  await expect(page.locator(".cc-main")).toHaveCSS("transform", "none");

  await swipe(8, 200);
  await expect(projects).toHaveAttribute("aria-current", "true");
});
