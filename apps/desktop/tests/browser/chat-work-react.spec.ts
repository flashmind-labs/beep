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
  const history = [{ role: "user", text: "Review this", at: 0, images: [], model: "", thinking: "" }];
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, open_session: history, session_agents: [], session_files: [{ message: 0, files: [{ id: "file-1", name: "example.txt", mime: "text/plain", size: 6 }] }], session_sources: [], list_questions: [] });
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
  const history = [{ role: "user", text: "Show an image", at: 1, images: [], model: "", thinking: "" }, { role: "assistant", html: "<p>Here.</p>", tools: [{ name: "send_image", label: "Show image", success: true, route: null, file: null, images: [{ url: image, caption: "Example" }, { url: image, caption: "Second" }], products: null, cards: null, chart: null }], replies: [], model: "" }];
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, open_session: history, session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.evaluate(() => window.__navigation.open({ chat: { session: "chat-example" } }));
  const picture = page.locator(".drpics button.pic").first();
  await expect(picture).toBeVisible();
  await expect(page.locator(".drpic").first()).not.toHaveClass(/wait/);
  await picture.focus();
  await picture.evaluate(el => Reflect.set(window, "replyPicture", el));
  await page.evaluate(() => window.__navigation.emit("sessions-changed", "chat-example"));
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "open_session").length)).toBeGreaterThan(1);
  expect(await picture.evaluate(el => el === Reflect.get(window, "replyPicture") && document.activeElement === el)).toBe(true);
  await picture.click();
  await expect(page.locator("#viewer")).toBeVisible();
  await expect(page.locator("#viewer .cap")).toHaveText("Example");
  await expect(page.locator("#viewer [data-v=close]")).toBeFocused();
  await page.locator("#viewer").press("ArrowRight");
  await expect(page.locator("#viewer .cap")).toHaveText("Second");
  await page.locator("#viewer").press("Escape");
  await expect(page.locator("#viewer")).toHaveCount(0);
  await expect(picture).toBeFocused();
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

test("chart controls keep selection and focus during history refresh", async ({ page }) => {
  const chart = { title: "Example chart", kind: "line", x: "category", categories: ["One", "Two"], unit: "%", series: [{ name: "First", points: [{ x: 0, y: 1 }, { x: 1, y: 2 }] }, { name: "Second", points: [{ x: 0, y: 2 }, { x: 1, y: 3 }] }], stock: null, source: "Example" };
  const history = [{ role: "user", text: "Show a chart", at: 1, images: [], model: "", thinking: "" }, { role: "assistant", html: "<p>Here.</p>", tools: [{ name: "show_chart", label: "Show chart", success: true, route: null, file: null, images: [], products: null, cards: null, chart }], replies: [], model: "" }];
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, open_session: history, session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.evaluate(() => window.__navigation.open({ chat: { session: "chat-example" } }));
  const plot = page.locator(".dchart .plot");
  const path = plot.locator("svg > path").first();
  await expect(path).toBeVisible();
  await path.evaluate(el => Reflect.set(window, "chartPath", el));
  await plot.hover({ position: { x: 20, y: 30 } });
  await expect(plot.locator(".tipc")).toBeVisible();
  await expect(plot.locator(".hov circle")).toHaveCount(2);
  await plot.hover({ position: { x: 80, y: 30 } });
  expect(await path.evaluate(el => el === Reflect.get(window, "chartPath"))).toBe(true);
  const button = page.locator(".dchart [data-series='0']");
  await expect(button).toHaveAttribute("aria-pressed", "true");
  await button.evaluate(el => Reflect.set(window, "chartResizeLegend", el));
  await plot.evaluate(el => { if (el instanceof HTMLElement) el.style.width = "220px"; });
  await expect(plot.locator("svg")).toHaveAttribute("width", "220");
  expect(await button.evaluate(el => el === Reflect.get(window, "chartResizeLegend"))).toBe(true);
  await button.click();
  await expect(button).toHaveAttribute("aria-pressed", "false");
  await button.evaluate(el => Reflect.set(window, "chartLegend", el));
  await page.evaluate(() => window.__navigation.emit("sessions-changed", "chat-example"));
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "open_session").length)).toBeGreaterThan(1);
  expect(await button.evaluate(el => el === Reflect.get(window, "chartLegend") && document.activeElement === el)).toBe(true);
  await expect(button).toHaveAttribute("aria-pressed", "false");
});

test("stock chart changes range without replacing controls", async ({ page }) => {
  const stock = { symbol: "TEST", name: "Example", exchange: "EX", currency: "USD", range: "1d", last: 2, previous: 1 };
  const chart = { title: "Example chart", kind: "line", x: "category", categories: ["One", "Two"], unit: "USD", series: [{ name: "First", points: [{ x: 0, y: 1 }, { x: 1, y: 2 }] }], stock, source: "Example" };
  const history = [{ role: "user", text: "Show a chart", at: 1, images: [], model: "", thinking: "" }, { role: "assistant", html: "<p>Here.</p>", tools: [{ name: "show_chart", label: "Show chart", success: true, route: null, file: null, images: [], products: null, cards: null, chart }], replies: [], model: "" }];
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, open_session: history, stock_chart: { ...chart, stock: { ...stock, range: "5d" } }, session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.evaluate(() => window.__navigation.open({ chat: { session: "chat-example" } }));
  const button = page.locator(".dchart [data-range='5d']");
  await button.click();
  await expect(button).toHaveAttribute("aria-pressed", "true");
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "stock_chart").map(call => call.args.range))).toEqual(["5d"]);
  await expect(button).toBeFocused();
});

test("picture save notice remains after closing the viewer", async ({ page }) => {
  const picture = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4z8DwHwAFgAI/ScLttAAAAABJRU5ErkJggg==";
  const history = [{ role: "user", text: "Show a picture", at: 1, images: [], model: "", thinking: "" }, { role: "assistant", html: "<p>Here.</p>", tools: [{ name: "send_image", label: "Show image", success: true, route: null, file: null, images: [{ file: { id: "image-1", name: "example.png", mime: "image/png", size: 10 }, caption: "Example" }], products: null, cards: null, chart: null }], replies: [], model: "" }];
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, open_session: history, picture, save_file: "/path/to/example.png", session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.evaluate(() => window.__navigation.open({ chat: { session: "chat-example" } }));
  await page.locator(".drpics button.pic").click();
  await page.locator("#viewer [data-v=save]").click();
  await expect(page.locator("#saved")).toContainText("Saved example.png");
  await page.locator("#viewer [data-v=close]").click();
  await expect(page.locator("#saved")).toBeVisible();
  await page.locator("#saved").getByRole("button", { name: "Show in Finder" }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "reveal_file" && call.args.path === "/path/to/example.png"))).toBe(true);
});

test("history removes obsolete turns without disturbing kept turns", async ({ page }) => {
  const first = { role: "user", text: "First", at: 1, images: [], model: "", thinking: "" };
  const second = { role: "user", text: "Second", at: 2, images: [], model: "", thinking: "" };
  const answer = { role: "assistant", html: "<p>Answer</p>", tools: [], replies: [], model: "" };
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, open_session: [first, answer, second, answer], session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.evaluate(() => window.__navigation.open({ chat: { session: "chat-example" } }));
  const kept = page.locator('.turn[data-at="2"]');
  await expect(kept).toContainText("Second");
  await kept.evaluate(el => Reflect.set(window, "keptConversationTurn", el));
  await page.evaluate(data => { window.__navigation.respond("open_session", data); window.__navigation.emit("sessions-changed", "chat-example"); }, [second, answer]);
  await expect(page.locator('.turn[data-at="1"]')).toHaveCount(0);
  expect(await kept.evaluate(el => el === Reflect.get(window, "keptConversationTurn"))).toBe(true);
});

test("streamed tool status updates retain the same row", async ({ page }) => {
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, open_session: [], session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.evaluate(() => {
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      if (command === "send_prompt" && args?.onEvent && typeof args.onEvent === "object" && "onmessage" in args.onEvent) {
        const channel = args.onEvent;
        Reflect.set(window, "toolEvent", (event: unknown) => { if (typeof channel.onmessage === "function") channel.onmessage(event); });
        return new Promise<T>(() => {});
      }
      return invoke<T>(command, args);
    };
  });
  await page.evaluate(() => window.__navigation.open({ chat: { session: "live-chat" } }));
  await page.locator("#ask").fill("Search");
  await page.locator("#ask").press("Enter");
  await expect.poll(() => page.evaluate(() => typeof Reflect.get(window, "toolEvent"))).toBe("function");
  await page.evaluate(() => Reflect.get(window, "toolEvent")({ kind: "tool_started", call_id: "call-1", name: "web_search", description: "Search example" }));
  const row = page.locator(".tools .tool.row");
  await expect(row.locator(".dspin")).toBeVisible();
  await row.evaluate(el => Reflect.set(window, "toolRow", el));
  await page.evaluate(() => Reflect.get(window, "toolEvent")({ kind: "tool_result", call_id: "call-1", name: "web_search", success: true, file: null, images: [], products: null, cards: null, chart: null }));
  await expect(row.locator(".dok")).toBeVisible();
  expect(await row.evaluate(el => el === Reflect.get(window, "toolRow"))).toBe(true);
});

test("streamed thought updates keep the thought body mounted", async ({ page }) => {
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, open_session: [], session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.evaluate(() => {
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      if (command === "send_prompt" && args?.onEvent && typeof args.onEvent === "object" && "onmessage" in args.onEvent) {
        const channel = args.onEvent;
        Reflect.set(window, "thoughtEvent", (event: unknown) => { if (typeof channel.onmessage === "function") channel.onmessage(event); });
        return new Promise<T>(() => {});
      }
      return invoke<T>(command, args);
    };
  });
  await page.evaluate(() => window.__navigation.open({ chat: { session: "live-chat" } }));
  await page.locator("#ask").fill("Think");
  await page.locator("#ask").press("Enter");
  await expect.poll(() => page.evaluate(() => typeof Reflect.get(window, "thoughtEvent"))).toBe("function");
  await page.evaluate(() => Reflect.get(window, "thoughtEvent")({ kind: "reasoning", delta: "First thought" }));
  const thought = page.locator(".tools .kk[data-kk]");
  await expect(thought).toContainText("First thought");
  await thought.evaluate(el => Reflect.set(window, "thoughtBody", el));
  await page.evaluate(() => Reflect.get(window, "thoughtEvent")({ kind: "reasoning", delta: " continues" }));
  await expect(thought).toContainText("First thought continues");
  expect(await thought.evaluate(el => el === Reflect.get(window, "thoughtBody"))).toBe(true);
});

test("history can add prompt pictures to an existing React turn", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", error => errors.push(error.message));
  page.on("console", message => { if (message.type() === "error") errors.push(message.text()); });
  const user = { role: "user", text: "See this", at: 1, images: [], model: "", thinking: "" };
  const answer = { role: "assistant", html: "<p>Done.</p>", tools: [], replies: [], model: "" };
  const picture = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4z8DwHwAFgAI/ScLttAAAAABJRU5ErkJggg==";
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, open_session: [user, answer], picture, session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.evaluate(() => window.__navigation.open({ chat: { session: "chat-example" } }));
  const turn = page.locator(".turn");
  await turn.evaluate(el => Reflect.set(window, "keptTurnWithPicture", el));
  const updated = [{ ...user, images: [{ id: "image-1", mime: "image/png" }] }, answer];
  await page.evaluate(data => { window.__navigation.respond("open_session", data); window.__navigation.emit("sessions-changed", "chat-example"); }, updated);
  await expect(turn.locator(".dpics img")).toHaveAttribute("src", picture);
  expect(await turn.evaluate(el => el === Reflect.get(window, "keptTurnWithPicture"))).toBe(true);
  await page.evaluate(data => { window.__navigation.respond("open_session", data); window.__navigation.emit("sessions-changed", "chat-example"); }, [updated[0], { ...answer, html: "<p>Updated.</p>" }]);
  await expect(turn.locator(".dcoderen.agent")).toContainText("Updated.");
  await expect(turn.locator(".dpics img")).toHaveCount(1);
  expect(errors).toEqual([]);
});

test("route selection and stop expansion survive history updates", async ({ page }) => {
  const leg = { mode: "bus", line: "1", name: "Bus", color: "", text_color: "", headsign: "", from: "Start", to: "End", from_track: "", to_track: "", departs: "10:00", arrives: "10:10", minutes: 10, stops: 1, meters: 0, points: [], between: [{ name: "Middle", code: "M", time: "10:05", at: { latitude: 0, longitude: 0 } }] };
  const route = { routes: [{ departs: "10:00", arrives: "10:10", minutes: 10, transfers: 0, live: true, legs: [leg] }, { departs: "10:15", arrives: "10:30", minutes: 15, transfers: 0, live: false, legs: [leg] }] };
  const user = { role: "user", text: "Find a route", at: 1, images: [], model: "", thinking: "" };
  const answer = { role: "assistant", html: "<p>Here.</p>", tools: [{ name: "get_route", label: "Find route", success: true, route: "route-1", file: null, images: [], products: null, cards: null, chart: null }], replies: [], model: "" };
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, open_session: [user, answer], route_card: route, session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.evaluate(() => window.__navigation.open({ chat: { session: "chat-example" } }));
  const card = page.locator(".droute");
  await card.getByRole("button", { name: "15 min" }).click();
  const stops = card.locator("[data-stops]");
  await stops.click();
  await expect(stops).toHaveAttribute("aria-expanded", "true");
  await expect(card).toContainText("Middle");
  await stops.evaluate(el => Reflect.set(window, "routeStops", el));
  await page.evaluate(data => { window.__navigation.respond("open_session", data); window.__navigation.emit("sessions-changed", "chat-example"); }, [user, { ...answer, html: "<p>Updated.</p>" }]);
  await expect(page.locator(".dcoderen.agent")).toContainText("Updated.");
  expect(await stops.evaluate(el => el === Reflect.get(window, "routeStops") && document.activeElement === el)).toBe(true);
  await expect(stops).toHaveAttribute("aria-expanded", "true");
  await expect(card.getByRole("button", { name: "15 min" })).toHaveAttribute("aria-pressed", "true");
});

test("bar chart hover tracks values and legend changes without replacing the plot", async ({ page }) => {
  const chart = { title: "Example bars", kind: "bar", x: "category", categories: ["One", "Two"], unit: "%", series: [{ name: "First", points: [{ x: 0, y: -1 }, { x: 1, y: 2 }] }, { name: "Second", points: [{ x: 0, y: 2 }, { x: 1, y: 3 }] }], stock: null, source: "" };
  const history = [{ role: "user", text: "Show bars", at: 1, images: [], model: "", thinking: "" }, { role: "assistant", html: "<p>Here.</p>", tools: [{ name: "show_chart", label: "Show chart", success: true, route: null, file: null, images: [], products: null, cards: null, chart }], replies: [], model: "" }];
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, open_session: history, session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.evaluate(() => window.__navigation.open({ chat: { session: "chat-example" } }));
  const plot = page.locator(".dchart .plot");
  await expect(plot.locator("rect")).toHaveCount(4);
  await plot.hover({ position: { x: 20, y: 30 } });
  await expect(plot.locator(".tipc")).toContainText("First");
  await expect(plot.locator(".tipc")).toContainText("Second");
  await expect(plot.locator(".hov circle")).toHaveCount(0);
  await page.locator(".dchart [data-series='0']").click();
  await expect(plot.locator("rect")).toHaveCount(2);
  await expect(plot.locator(".tipc")).toBeHidden();
});

test("area charts keep separate gradients and resize without replacing plot controls", async ({ page }) => {
  const makeChart = (title: string) => ({ title, kind: "area", x: "category", categories: ["One", "Two"], unit: "%", series: [{ name: "First", points: [{ x: 0, y: 1 }, { x: 1, y: 2 }] }], stock: null, source: "" });
  const tool = (chart: ReturnType<typeof makeChart>) => ({ name: "show_chart", label: "Show chart", success: true, route: null, file: null, images: [], products: null, cards: null, chart });
  const history = [{ role: "user", text: "Show charts", at: 1, images: [], model: "", thinking: "" }, { role: "assistant", html: "<p>Here.</p>", tools: [tool(makeChart("One")), tool(makeChart("Two"))], replies: [], model: "" }];
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, open_session: history, session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.evaluate(() => window.__navigation.open({ chat: { session: "chat-example" } }));
  const plots = page.locator(".dchart .plot");
  await expect(plots).toHaveCount(2);
  const ids = await plots.locator("linearGradient").evaluateAll(elements => elements.map(element => element.id));
  expect(ids).toHaveLength(2);
  expect(ids[0]).not.toBe(ids[1]);
  for (let i = 0; i < 2; i++) await expect(plots.nth(i).locator("path").first()).toHaveAttribute("fill", `url(#${ids[i]})`);
  const first = plots.first(), svg = first.locator("svg"), oldWidth = await svg.getAttribute("width");
  await svg.evaluate(el => Reflect.set(window, "chartSvg", el));
  await first.evaluate(el => { if (el instanceof HTMLElement) el.style.width = "220px"; });
  await expect(svg).toHaveAttribute("width", "220");
  expect(oldWidth).not.toBe("220");
  expect(await svg.evaluate(el => el === Reflect.get(window, "chartSvg"))).toBe(true);
});

test("thought and steps toggle through React handlers and keep their rows", async ({ page }) => {
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, open_session: [], session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.evaluate(() => {
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      if (command === "send_prompt" && args?.onEvent && typeof args.onEvent === "object" && "onmessage" in args.onEvent) {
        const channel = args.onEvent;
        Reflect.set(window, "workEvent", (event: unknown) => { if (typeof channel.onmessage === "function") channel.onmessage(event); });
        return new Promise<T>(() => {});
      }
      return invoke<T>(command, args);
    };
  });
  await page.evaluate(() => window.__navigation.open({ chat: { session: "live-chat" } }));
  await page.locator("#ask").fill("Think and search");
  await page.locator("#ask").press("Enter");
  await expect.poll(() => page.evaluate(() => typeof Reflect.get(window, "workEvent"))).toBe("function");
  await page.evaluate(() => {
    const send = Reflect.get(window, "workEvent");
    send({ kind: "reasoning", delta: "First thought" });
    send({ kind: "tool_started", call_id: "call-1", name: "web_search", description: "Search example" });
    send({ kind: "tool_started", call_id: "call-2", name: "web_search", description: "Search more" });
  });

  const thought = page.locator(".tools .dthink:not(.dlive) > button");
  const box = page.locator(".tools .dthink:not(.dlive)");
  await expect(thought).toHaveAttribute("aria-expanded", "false");
  await thought.evaluate(el => Reflect.set(window, "thoughtButton", el));
  await thought.click();
  await expect(thought).toHaveAttribute("aria-expanded", "true");
  await expect(box).toHaveClass(/open/);
  await thought.click();
  await expect(thought).toHaveAttribute("aria-expanded", "false");
  await expect(box).not.toHaveClass(/open/);
  expect(await thought.evaluate(el => el === Reflect.get(window, "thoughtButton"))).toBe(true);

  const steps = page.locator(".tools .hd[data-turn]");
  const rows = page.locator(".tools .tool.row");
  await expect(rows).toHaveCount(1);
  await steps.click();
  await expect(steps).toHaveAttribute("aria-expanded", "true");
  await expect(rows).toHaveCount(2);
  await steps.click();
  await expect(steps).toHaveAttribute("aria-expanded", "false");
  await expect(rows).toHaveCount(1);
});
