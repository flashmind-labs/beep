import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

test("chat work keeps its heading and fold state on history refresh", async ({ page }) => {
  const id = 'chat-example';
  const history = [{ role: 'user', text: 'Explain this', at: 1, images: [], model: '', thinking: '' }, { role: 'assistant', html: '<p>Done.</p>', tools: [{ name: 'web_search', label: 'Search example', success: true, route: null, file: null, images: [], products: null, cards: null, chart: null }], replies: [], model: '' }];
  await openDesktop(page, false, { status: { link: 'linked', signed_out: false, server: true, live: false, session: null }, open_session: history, session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.evaluate(session => window.__navigation.open({ chat: { session } }), id);
  const heading = page.locator('#body .tools .hd[data-turn]');
  await expect(heading).toBeVisible();
  await heading.focus();
  await heading.evaluate(el => { el.dataset.identity = 'kept'; });
  await heading.press('Enter');
  await expect(heading).toHaveAttribute('aria-expanded', 'true');
  await expect(page.locator('#body .tools .tool')).toContainText('Search');
  await page.evaluate(session => window.__navigation.emit('sessions-changed', session), id);
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === 'open_session').length)).toBeGreaterThan(1);
  await expect(heading).toHaveAttribute('data-identity', 'kept');
  await expect(heading).toHaveAttribute('aria-expanded', 'true');
  await expect(heading).toBeFocused();
});


test("prompt files keep their controls through history updates", async ({ page }) => {
  const history = [{ role: "user", text: "Review this", at: 1, images: [], model: "", thinking: "" }];
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, open_session: history, session_agents: [], session_files: [{ message: 1, files: [{ id: "file-1", name: "example.txt", mime: "text/plain", size: 6 }] }], session_sources: [], list_questions: [] });
  await page.evaluate(() => window.__navigation.open({ chat: { session: "chat-example" } }));
  const chip = page.locator(".dachips button");
  await expect(chip).toContainText("example.txt");
  await chip.focus();
  await chip.evaluate(el => Reflect.set(window, "fileChip", el));
  await page.evaluate(() => window.__navigation.emit("sessions-changed", "chat-example"));
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "open_session").length)).toBeGreaterThan(1);
  expect(await chip.evaluate(el => el === Reflect.get(window, "fileChip") && document.activeElement === el)).toBe(true);
  await chip.click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "open_file" && call.args.id === "file-1"))).toBe(true);
});

test("prompt pictures load and stay mounted through history updates", async ({ page }) => {
  const history = [{ role: "user", text: "See this", at: 1, images: [{ id: "picture-1", mime: "image/png" }], model: "", thinking: "" }];
  const picture = "data:image/png;base64,aGVsbG8=";
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, open_session: history, picture, session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.evaluate(() => window.__navigation.open({ chat: { session: "chat-example" } }));
  const image = page.locator(".dpics img");
  await expect(image).toHaveAttribute("src", picture);
  await image.evaluate(el => Reflect.set(window, "promptPicture", el));
  await page.evaluate(() => window.__navigation.emit("sessions-changed", "chat-example"));
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "open_session").length)).toBeGreaterThan(1);
  expect(await image.evaluate(el => el === Reflect.get(window, "promptPicture"))).toBe(true);
});
