import { expect, test, type Page } from "@playwright/test";
import { openDesktop, projectName } from "./harness.ts";
import type { CenterTarget } from "../../src/project/projectwindow.ts";

/// Runs `step` and returns where the page animation it started begins: "fade" when it fades in place.
function motionOf(page: Page, step: CenterTarget | "back"): Promise<string | null> {
  return page.evaluate(async step => {
    if (step === "back") document.querySelector<HTMLElement>(".mb-back")?.click();
    else window.__navigation.open(step);
    // Navigation lands a moment later; each animation runs at least 160 ms.
    await new Promise(done => setTimeout(done, 30));
    const frame = document.getAnimations().flatMap(a => a.effect instanceof KeyframeEffect && a.effect.target?.matches(".cc-box") ? [a.effect.getKeyframes()[0]] : []).at(-1);
    if (!frame) return null;
    return typeof frame.transform === "string" ? frame.transform : "fade";
  }, step);
}

test("desktop pages fade in", async ({ page }) => {
  await openDesktop(page);
  expect(await motionOf(page, "instructions")).toBe("fade");
});

test("phone pages slide in, slide back on Back, and fade between tabs", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await openDesktop(page);
  await expect(page.locator("body")).toHaveClass(/mobile/);
  expect(await motionOf(page, "terminals")).toBe("fade");
  expect(await motionOf(page, "instructions")).toBe("translateX(28%)");
  await expect(page.locator(".mb-back")).toBeVisible();
  expect(await motionOf(page, "back")).toBe("translateX(-22%)");
});

test("Reduce motion turns page animations off", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openDesktop(page);
  expect(await motionOf(page, "instructions")).toBeNull();
});

test("opening a session and going back capture only the card and the head", async ({ page }) => {
  // A captured page is painted again before the first frame, which stalls a heavy session.
  await page.addInitScript(() => {
    const start = document.startViewTransition.bind(document), moves: string[][] = [];
    Reflect.set(window, "moves", moves);
    document.startViewTransition = (update?: ViewTransitionUpdateCallback) => {
      const transition = start(update);
      void transition.ready.then(() => moves.push(document.getAnimations().flatMap(a => a.effect instanceof KeyframeEffect && a.effect.pseudoElement?.startsWith("::view-transition-group") ? [a.effect.pseudoElement] : [])));
      return transition;
    };
  });
  await openDesktop(page, true);
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  const card = page.locator(".bd-card:visible").filter({ hasText: "First notification" });
  const moves = () => page.evaluate(() => Reflect.get(window, "moves"));

  await card.click();
  await expect(page.locator("#agmsg")).toBeVisible();
  await expect.poll(moves).toEqual([["::view-transition-group(board-hero)"]]);
  await expect(page.locator("html")).not.toHaveAttribute("data-move");
  await page.keyboard.press("Escape");
  await expect(card).toBeVisible();
  await expect.poll(moves).toEqual([["::view-transition-group(board-hero)"], ["::view-transition-group(board-hero)"]]);
});

test("a two-finger swipe goes back and forward, and a sideways scroller keeps it", async ({ page }) => {
  await openDesktop(page);
  const title = page.locator("#title");
  const home = await title.textContent();
  await page.evaluate(() => window.__navigation.open("instructions"));
  await expect(title).not.toHaveText(home ?? "");
  const swipe = async (by: number) => {
    // The wheel rests between two swipes.
    await page.waitForTimeout(250);
    for (let i = 0; i < 4; i++) await page.mouse.wheel(by, 0);
  };
  await page.mouse.move(700, 450);

  // Fingers stopping short leave the page where it was.
  await page.waitForTimeout(250);
  await page.mouse.wheel(-60, 0);
  await expect(page.locator(".cc-main")).toHaveAttribute("style", /translateX\(60px\)/);
  await expect(page.locator(".cc-main")).not.toHaveAttribute("style", /translateX/);
  await expect(title).not.toHaveText(home ?? "");

  await swipe(-50);
  await expect(title).toHaveText(home ?? "");
  await swipe(50);
  await expect(title).not.toHaveText(home ?? "");

  await page.evaluate(() => {
    const scroller = document.createElement("div");
    scroller.style.cssText = "position:fixed;left:600px;top:400px;width:200px;height:100px;overflow-x:auto;z-index:99";
    scroller.innerHTML = '<div style="width:2000px;height:50px"></div>';
    scroller.id = "sideways";
    document.querySelector(".cc-main")?.append(scroller);
    scroller.scrollLeft = 500;
  });
  await swipe(-50);
  await expect.poll(() => page.locator("#sideways").evaluate(el => el.scrollLeft)).toBe(300);
  await expect(title).not.toHaveText(home ?? "");
});
