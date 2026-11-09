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

test("reply files stay mounted during refresh and open on request", async ({ page }) => {
  const history = [{ role: "user", text: "Make a file", at: 1, images: [], model: "", thinking: "" }, { role: "assistant", html: "<p>Done.</p>", tools: [{ name: "send_file", label: "Send file", success: true, route: null, file: { id: "result-1", name: "result.txt", mime: "text/plain", size: 6 }, images: [], products: null, cards: null, chart: null }], replies: [], model: "" }];
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, open_session: history, session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.evaluate(() => window.__navigation.open({ chat: { session: "chat-example" } }));
  const card = page.locator(".dfile");
  await expect(card).toContainText("result.txt");
  await card.focus();
  await card.evaluate(el => Reflect.set(window, "replyFile", el));
  await page.evaluate(() => window.__navigation.emit("sessions-changed", "chat-example"));
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "open_session").length)).toBeGreaterThan(1);
  expect(await card.evaluate(el => el === Reflect.get(window, "replyFile") && document.activeElement === el)).toBe(true);
  await card.click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "open_file" && call.args.id === "result-1"))).toBe(true);
});

test("reply pictures keep their viewer actions after a history refresh", async ({ page }) => {
  const image = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4z8DwHwAFgAI/ScLttAAAAABJRU5ErkJggg==";
  const history = [{ role: "user", text: "Show an image", at: 1, images: [], model: "", thinking: "" }, { role: "assistant", html: "<p>Here.</p>", tools: [{ name: "send_image", label: "Show image", success: true, route: null, file: null, images: [{ url: image, caption: "Example" }], products: null, cards: null, chart: null }], replies: [], model: "" }];
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, open_session: history, session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.evaluate(() => window.__navigation.open({ chat: { session: "chat-example" } }));
  const picture = page.locator(".drpics button.pic");
  await expect(picture).toBeVisible();
  await expect(page.locator(".drpic")).not.toHaveClass(/wait/);
  await picture.focus();
  await picture.evaluate(el => Reflect.set(window, "replyPicture", el));
  await page.evaluate(() => window.__navigation.emit("sessions-changed", "chat-example"));
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "open_session").length)).toBeGreaterThan(1);
  expect(await picture.evaluate(el => el === Reflect.get(window, "replyPicture") && document.activeElement === el)).toBe(true);
  await picture.click();
  await expect(page.locator("#viewer")).toBeVisible();
  await expect(page.locator("#viewer .cap")).toHaveText("Example");
});

test("reply product links keep delegated actions after refresh", async ({ page }) => {
  const history = [{ role: "user", text: "Find a product", at: 1, images: [], model: "", thinking: "" }, { role: "assistant", html: "<p>Found one.</p>", tools: [{ name: "show_products", label: "Show products", success: true, route: null, file: null, images: [], products: '<div class="dprods"><div class="dprod"><a class="dprod-card" href="https://example.com/item"><span class="dprod-img"><img src="data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4z8DwHwAFgAI/ScLttAAAAABJRU5ErkJggg=="></span>Example</a></div></div>', cards: null, chart: null }], replies: [], model: "" }];
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, open_session: history, session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.evaluate(() => window.__navigation.open({ chat: { session: "chat-example" } }));
  const link = page.locator(".dprod-card");
  await expect(link).toHaveText("Example");
  await expect(page.locator(".dprod-img img")).toHaveClass(/on/);
  await link.focus();
  await link.evaluate(el => Reflect.set(window, "productLink", el));
  await page.evaluate(() => window.__navigation.emit("sessions-changed", "chat-example"));
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "open_session").length)).toBeGreaterThan(1);
  expect(await link.evaluate(el => el === Reflect.get(window, "productLink") && document.activeElement === el)).toBe(true);
  await link.click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "open_url" && call.args.url === "https://example.com/item"))).toBe(true);
});

test("live chat notices show errors and retain the retry action", async ({ page }) => {
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, open_session: [], session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.evaluate(() => {
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      if (command === "send_prompt" && args?.onEvent && typeof args.onEvent === "object" && "onmessage" in args.onEvent) {
        const channel = args.onEvent;
        Reflect.set(window, "noticeEvent", (event: unknown) => { if (typeof channel.onmessage === "function") channel.onmessage(event); });
        return new Promise<T>(() => {});
      }
      return invoke<T>(command, args);
    };
  });
  await page.evaluate(() => window.__navigation.open({ chat: { session: "live-chat" } }));
  await page.locator("#ask").fill("Try this");
  await page.locator("#ask").press("Enter");
  await expect.poll(() => page.evaluate(() => typeof Reflect.get(window, "noticeEvent"))).toBe("function");
  await page.evaluate(() => { const send = Reflect.get(window, "noticeEvent"); send({ kind: "auto_picked", model: "example-model", thinking: "high" }); send({ kind: "error", message: "Example failure" }); send({ kind: "finished", status: "cancelled" }); });
  await expect(page.locator(".dranon")).toContainText("high");
  await expect(page.locator(".derrcard")).toContainText("Example failure");
  const retry = page.locator(".dstopped").getByRole("button", { name: "Retry" });
  await expect(retry).toBeVisible();
  await retry.click();
  await expect(page.locator(".dstopped")).toHaveCount(0);
});

test("scheduled turn labels stay mounted during history refresh", async ({ page }) => {
  const history = [{ role: "scheduled", label: "Daily check", at: 1 }, { role: "assistant", html: "<p>Done.</p>", tools: [], replies: [], model: "" }];
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, open_session: history, session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.evaluate(() => window.__navigation.open({ chat: { session: "chat-example" } }));
  const label = page.locator(".turn .dsystem");
  await expect(label).toHaveText("Daily check");
  await label.evaluate(el => Reflect.set(window, "scheduledLabel", el));
  await page.evaluate(() => window.__navigation.emit("sessions-changed", "chat-example"));
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "open_session").length)).toBeGreaterThan(1);
  expect(await label.evaluate(el => el === Reflect.get(window, "scheduledLabel"))).toBe(true);
});

test("live approval shows its answer after the card is removed", async ({ page }) => {
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, open_session: [], session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.evaluate(() => {
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      if (command === "send_prompt" && args?.onEvent && typeof args.onEvent === "object" && "onmessage" in args.onEvent) {
        const channel = args.onEvent;
        Reflect.set(window, "approvalEvent", (event: unknown) => { if (typeof channel.onmessage === "function") channel.onmessage(event); });
        return new Promise<T>(() => {});
      }
      return invoke<T>(command, args);
    };
  });
  await page.evaluate(() => window.__navigation.open({ chat: { session: "live-chat" } }));
  await page.locator("#ask").fill("Do this");
  await page.locator("#ask").press("Enter");
  await expect.poll(() => page.evaluate(() => typeof Reflect.get(window, "approvalEvent"))).toBe("function");
  await page.evaluate(() => Reflect.get(window, "approvalEvent")({ kind: "approval", query_id: "query-1", app: "Example", action: "Update", fields: [] }));
  await page.locator(".dcard").getByRole("button", { name: /^Allow for this session/ }).click();
  await expect(page.locator(".turn .dmuted")).toContainText("Allowed for the rest of this chat");
  await expect(page.locator(".dcard")).toHaveCount(0);
});
