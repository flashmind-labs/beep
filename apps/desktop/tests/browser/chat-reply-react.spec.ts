import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

test("streamed replies retain selection and host identity until selection clears", async ({ page }) => {
  await openDesktop(page, false, { status: { link: 'linked', signed_out: false, server: true, live: false, session: null }, open_session: [], session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.evaluate(() => {
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      if (command === 'send_prompt' && args?.onEvent && typeof args.onEvent === 'object' && 'onmessage' in args.onEvent) {
        const channel = args.onEvent;
        Reflect.set(window, 'sendChatEvent', (event: unknown) => { if (typeof channel.onmessage === 'function') channel.onmessage(event); });
        return new Promise<T>(() => {});
      }
      return invoke<T>(command, args);
    };
  });
  await page.evaluate(() => window.__navigation.open({ chat: { session: 'live-chat' } }));
  await page.locator('#ask').fill('Explain');
  await page.locator('#ask').press('Enter');
  await expect.poll(() => page.evaluate(() => typeof Reflect.get(window, 'sendChatEvent'))).toBe('function');
  await page.evaluate(() => Reflect.get(window, 'sendChatEvent')({ kind: 'text', html: '<p>First reply</p>' }));
  const reply = page.locator('.dcoderen.agent');
  await expect(reply).toContainText('First reply');
  await reply.evaluate(el => {
    el.setAttribute('data-identity', 'kept');
    const range = document.createRange();
    range.selectNodeContents(el.querySelector('p')!);
    const selection = window.getSelection();
    selection?.removeAllRanges();
    selection?.addRange(range);
  });
  await page.evaluate(() => Reflect.get(window, 'sendChatEvent')({ kind: 'text', html: '<p>Updated reply</p>' }));
  await page.evaluate(() => new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))));
  await expect(reply).toContainText('First reply');
  expect(await page.evaluate(() => window.getSelection()?.toString())).toBe('First reply');
  await page.evaluate(() => window.getSelection()?.removeAllRanges());
  await expect(reply).toContainText('Updated reply');
  await expect(reply).toHaveAttribute('data-identity', 'kept');
});

test("history refresh preserves selected reply and applies its update later", async ({ page }) => {
  const id = 'chat-example';
  const user = { role: 'user', text: 'Explain', at: 1, images: [], model: '', thinking: '' };
  const answer = { role: 'assistant', html: '<p>First answer</p>', tools: [], replies: [], model: '' };
  await openDesktop(page, false, { status: { link: 'linked', signed_out: false, server: true, live: false, session: null }, open_session: [user, answer], session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.evaluate(session => window.__navigation.open({ chat: { session } }), id);
  const reply = page.locator('.dcoderen.agent');
  await expect(reply).toContainText('First answer');
  await reply.evaluate(el => {
    el.setAttribute('data-identity', 'kept');
    const range = document.createRange();
    range.selectNodeContents(el.querySelector('p')!);
    window.getSelection()?.removeAllRanges();
    window.getSelection()?.addRange(range);
  });
  await page.evaluate(({ id, user, answer }) => {
    window.__navigation.respond('open_session', [user, { ...answer, html: '<p>Updated answer</p>' }]);
    window.__navigation.emit('sessions-changed', id);
  }, { id, user, answer });
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === 'open_session').length)).toBeGreaterThan(1);
  await expect(reply).toContainText('First answer');
  await page.evaluate(() => window.getSelection()?.removeAllRanges());
  await expect(reply).toContainText('Updated answer');
  await expect(reply).toHaveAttribute('data-identity', 'kept');
});

test("history combines assistant fragments and keeps code copy controls", async ({ page }) => {
  const user = { role: 'user', text: 'Explain', at: 1, images: [], model: '', thinking: '' };
  const assistant = { role: 'assistant', tools: [], replies: [], model: '' };
  const history = [user, { ...assistant, html: '<p>First fragment</p>' }, { ...assistant, html: '<pre><code>example</code></pre>' }];
  await openDesktop(page, false, { status: { link: 'linked', signed_out: false, server: true, live: false, session: null }, open_session: history, session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.evaluate(() => window.__navigation.open({ chat: { session: 'chat-example' } }));
  const reply = page.locator('.dcoderen.agent');
  await expect(reply.locator('p')).toHaveText('First fragment');
  await expect(reply.locator('pre code')).toHaveText('example');
  await expect(reply.locator('pre button[data-copy]')).toBeVisible();
});

test("transcript updates keep the React-owned user prompt through later events", async ({ page }) => {
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, open_session: [], session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.evaluate(() => {
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      if (command === "send_prompt" && args?.onEvent && typeof args.onEvent === "object" && "onmessage" in args.onEvent) {
        const channel = args.onEvent;
        Reflect.set(window, "promptEvents", (event: unknown) => { if (typeof channel.onmessage === "function") channel.onmessage(event); });
        return new Promise<T>(() => {});
      }
      return invoke<T>(command, args);
    };
  });
  await page.evaluate(() => window.__navigation.open({ chat: { session: "live-chat" } }));
  await page.locator("#ask").fill("Original question");
  await page.locator("#ask").press("Enter");
  await expect.poll(() => page.evaluate(() => typeof Reflect.get(window, "promptEvents"))).toBe("function");
  const prompt = page.locator(".dcoderen.user");
  await expect(prompt).toContainText("Original question");
  await prompt.evaluate(el => { const range = document.createRange(); range.selectNodeContents(el.querySelector("span")!); window.getSelection()?.removeAllRanges(); window.getSelection()?.addRange(range); });
  await page.evaluate(() => Reflect.get(window, "promptEvents")({ kind: "transcript", text: "Transcribed question" }));
  await expect(prompt).toContainText("Original question");
  expect(await page.evaluate(() => window.getSelection()?.toString())).toBe("Original question");
  await page.evaluate(() => window.getSelection()?.removeAllRanges());
  await expect(prompt).toContainText("Transcribed question");
  await page.evaluate(() => Reflect.get(window, "promptEvents")({ kind: "text", html: "<p>Reply</p>" }));
  await expect(page.locator(".dcoderen.agent")).toContainText("Reply");
  await expect(prompt).toContainText("Transcribed question");
});

test("history refresh keeps a selected user prompt while answers update", async ({ page }) => {
  const user = { role: "user", text: "A selected prompt", at: 1, images: [], model: "", thinking: "" };
  const answer = { role: "assistant", html: "<p>First answer</p>", tools: [], replies: [], model: "" };
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, open_session: [user, answer], session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.evaluate(() => window.__navigation.open({ chat: { session: "chat-example" } }));
  const prompt = page.locator(".dcoderen.user");
  await expect(prompt).toContainText("A selected prompt");
  await prompt.evaluate(el => { Reflect.set(window, "keptPrompt", el); const range = document.createRange(); range.selectNodeContents(el.querySelector("span")!); window.getSelection()?.removeAllRanges(); window.getSelection()?.addRange(range); });
  await page.evaluate(data => { window.__navigation.respond("open_session", data); window.__navigation.emit("sessions-changed", "chat-example"); }, [user, { ...answer, html: "<p>Updated answer</p>" }]);
  await expect(page.locator(".dcoderen.agent")).toContainText("Updated answer");
  expect(await prompt.evaluate(el => el === Reflect.get(window, "keptPrompt") && window.getSelection()?.toString() === "A selected prompt")).toBe(true);
});

test("follow-up suggestions retain focus through an unchanged history refresh", async ({ page }) => {
  const user = { role: "user", text: "Explain", at: 1, images: [], model: "", thinking: "" };
  const answer = { role: "assistant", html: "<p>Answer</p>", tools: [], replies: ["Tell me more", "Show an example"], model: "" };
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, open_session: [user, answer], session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.evaluate(() => window.__navigation.open({ chat: { session: "chat-example" } }));
  const button = page.getByRole("button", { name: "Tell me more" });
  await button.focus();
  await button.evaluate(el => Reflect.set(window, "followUpButton", el));
  const reads = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "open_session").length);
  await page.evaluate(() => window.__navigation.emit("sessions-changed", "chat-example"));
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "open_session").length)).toBeGreaterThan(reads);
  expect(await button.evaluate(el => el === Reflect.get(window, "followUpButton") && document.activeElement === el)).toBe(true);
});

test("changed follow-ups replace old actions and send the new choice", async ({ page }) => {
  const user = { role: "user", text: "Explain", at: 1, images: [], model: "", thinking: "" };
  const answer = { role: "assistant", html: "<p>Answer</p>", tools: [], replies: ["Old suggestion"], model: "" };
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, open_session: [user, answer], session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.evaluate(() => window.__navigation.open({ chat: { session: "chat-example" } }));
  await expect(page.getByRole("button", { name: "Old suggestion" })).toBeVisible();
  await page.evaluate(data => { window.__navigation.respond("open_session", data); window.__navigation.emit("sessions-changed", "chat-example"); }, [user, { ...answer, replies: ["New suggestion"] }]);
  const button = page.getByRole("button", { name: "New suggestion" });
  await expect(button).toBeVisible();
  await expect(page.getByRole("button", { name: "Old suggestion" })).toHaveCount(0);
  await button.evaluate(el => Reflect.set(window, "retiredSuggestions", el.closest(".dreplies")));
  await button.click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "send_prompt" && call.args.message === "New suggestion"))).toBe(true);
  await expect.poll(() => page.evaluate(() => { const node = Reflect.get(window, "retiredSuggestions"); return node instanceof HTMLElement ? node.childElementCount : -1; })).toBe(0);
});

test("approval cards remain stable during refresh and clean up on answer", async ({ page }) => {
  const user = { role: "user", text: "Please proceed", at: 1, images: [], model: "", thinking: "" };
  const approval = { role: "approval", query_id: "query-1", app: "Example app", action: "Update item", fields: [{ label: "Item", value: "Example" }] };
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, open_session: [user, approval], session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.evaluate(() => window.__navigation.open({ chat: { session: "chat-example" } }));
  const card = page.locator(".dcard");
  await expect(card).toContainText("Update item?");
  await expect(card).toContainText("Example");
  await card.evaluate(el => Reflect.set(window, "oldApproval", el));
  const reads = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "open_session").length);
  await page.evaluate(() => window.__navigation.emit("sessions-changed", "chat-example"));
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "open_session").length)).toBeGreaterThan(reads);
  expect(await card.evaluate(el => el === Reflect.get(window, "oldApproval"))).toBe(true);
  const current = page.locator(".dcard");
  await current.evaluate(el => Reflect.set(window, "answeredApproval", el));
  await current.getByRole("button", { name: /^Allow for this session/ }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "answer_approval").map(call => [call.args.queryId, call.args.decision]))).toEqual([["query-1", "session"]]);
  await expect.poll(() => page.evaluate(() => { const old = Reflect.get(window, "answeredApproval"); return old instanceof HTMLElement && !old.isConnected && old.childElementCount === 0; })).toBe(true);
});

test("an approval answered elsewhere releases its card", async ({ page }) => {
  const user = { role: "user", text: "Please proceed", at: 1, images: [], model: "", thinking: "" };
  const approval = { role: "approval", query_id: "query-2", app: "Example app", action: "Update item", fields: [] };
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, open_session: [user, approval], session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.evaluate(() => window.__navigation.open({ chat: { session: "chat-example" } }));
  const card = page.locator(".dcard");
  await expect(card).toBeVisible();
  await card.evaluate(el => Reflect.set(window, "remoteApproval", el));
  await page.evaluate(() => window.__navigation.emit("approval-answered", { session: "chat-example", query_id: "query-2" }));
  await expect(page.locator("#body")).toContainText("Answered");
  await expect.poll(() => page.evaluate(() => { const old = Reflect.get(window, "remoteApproval"); return old instanceof HTMLElement && !old.isConnected && old.childElementCount === 0; })).toBe(true);
});

test("reply actions retain focus during history refresh", async ({ page }) => {
  const user = { role: "user", text: "Explain", at: 1, images: [], model: "", thinking: "" };
  const answer = { role: "assistant", html: "<p>Answer</p>", tools: [], replies: [], model: "" };
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, open_session: [user, answer], session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.evaluate(() => window.__navigation.open({ chat: { session: "chat-example" } }));
  const copy = page.locator('.dreplyacts [data-act="copyReply"]');
  await expect(copy).toBeVisible();
  await copy.focus();
  await copy.evaluate(el => Reflect.set(window, "replyCopy", el));
  const reads = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "open_session").length);
  await page.evaluate(() => window.__navigation.emit("sessions-changed", "chat-example"));
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "open_session").length)).toBeGreaterThan(reads);
  expect(await copy.evaluate(el => el === Reflect.get(window, "replyCopy") && document.activeElement === el)).toBe(true);
});

test("rerunning a turn releases its detached React content", async ({ page }) => {
  const user = { role: "user", text: "Explain", at: 1, images: [], model: "", thinking: "" };
  const answer = { role: "assistant", html: "<p>Answer</p>", tools: [], replies: ["Follow up"], model: "one" };
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, get_models: { choices: [{ id: "one", name: "One", input_price: null, output_price: null, sees_images: false, default: true }], selected: "one", thinking: "", auto: false, auto_available: false }, open_session: [user, answer], session_agents: [], session_files: [], session_sources: [{ message: 1, sources: [{ url: "https://example.test/source", title: "Source" }] }], list_questions: [] });
  await page.evaluate(() => window.__navigation.open({ chat: { session: "chat-example" } }));
  const source = page.locator(".dsources");
  await expect(source).toBeVisible();
  await source.evaluate(el => Reflect.set(window, "oldSources", el));
  const actions = page.locator(".dreplyacts");
  await expect(actions.locator('[data-act="rerunMenu"]')).toBeVisible();
  await actions.evaluate(el => Reflect.set(window, "oldReplyActions", el));
  await actions.locator('[data-act="rerunMenu"]').click();
  await page.getByRole("menuitem", { name: /One/ }).click();
  await expect.poll(() => page.evaluate(() => { const node = Reflect.get(window, "oldReplyActions"); return node instanceof HTMLElement && !node.isConnected && node.childElementCount === 0; })).toBe(true);
  await expect.poll(() => page.evaluate(() => { const node = Reflect.get(window, "oldSources"); return node instanceof HTMLElement && !node.isConnected && node.childElementCount === 0; })).toBe(true);
});

test("source chips keep expansion and focus as source lists update", async ({ page }) => {
  const user = { role: "user", text: "Explain", at: 1, images: [], model: "", thinking: "" };
  const answer = { role: "assistant", html: "<p>Answer</p>", tools: [], replies: [], model: "" };
  const sources = Array.from({ length: 4 }, (_, i) => ({ url: `https://site${i}.example.test/article`, title: `Source ${i}` }));
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, open_session: [user, answer], session_agents: [], session_files: [], session_sources: [{ message: 1, sources }], list_questions: [] });
  await page.evaluate(() => window.__navigation.open({ chat: { session: "chat-example" } }));
  const row = page.locator(".dsources");
  await row.getByRole("button", { name: "Show 1 more sources" }).click();
  const first = row.getByRole("button", { name: "S site0.example.test" });
  await first.focus();
  await first.evaluate(el => Reflect.set(window, "sourceChip", el));
  await page.evaluate(value => { window.__navigation.respond("session_sources", [{ message: 1, sources: value }]); window.__navigation.emit("sessions-changed", "chat-example"); }, [...sources, { url: "https://site4.example.test/article", title: "Source 4" }]);
  await expect(row.getByRole("button", { name: "S site4.example.test" })).toBeVisible();
  expect(await first.evaluate(el => el === Reflect.get(window, "sourceChip") && document.activeElement === el)).toBe(true);
  await first.click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "open_url" && call.args.url === "https://site0.example.test/article"))).toBe(true);
});

test("rerun menu sends the selected model and thinking level", async ({ page }) => {
  const user = { role: "user", text: "Explain", at: 1, images: [], model: "", thinking: "" };
  const answer = { role: "assistant", html: "<p>Answer</p>", tools: [], replies: [], model: "one" };
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, get_models: { choices: [{ id: "one", name: "One", input_price: null, output_price: null, sees_images: false, default: true }], selected: "one", thinking: "", auto: false, auto_available: false }, open_session: [user, answer], session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.evaluate(() => window.__navigation.open({ chat: { session: "chat-example" } }));
  await page.locator('[data-act="rerunMenu"]').click();
  await page.locator('[data-rthink="high"]').click();
  await expect(page.locator('[data-rthink="high"]')).toHaveAttribute("aria-pressed", "true");
  await page.getByRole("menuitem", { name: /One/ }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "rerun_prompt").map(call => call.args.pick))).toEqual([{ model: "one", thinking: "high" }]);
});
