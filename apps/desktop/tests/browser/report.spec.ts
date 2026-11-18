import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

test("report sheet keeps a typed description and focus when the period changes", async ({ page }) => {
  await openDesktop(page);
  await page.evaluate(() => window.__navigation.open("report"));
  const sheet = page.getByRole("dialog", { name: "Report a problem" });
  const description = sheet.getByRole("textbox", { name: "What happened?" });
  await expect(description).toBeFocused();
  await description.fill("A sync stopped");
  await expect(description).toBeFocused();
  await sheet.getByRole("radio", { name: "Last hour" }).click();
  await expect(description).toHaveValue("A sync stopped");
  await expect(sheet.getByRole("radio", { name: "Last hour" })).toHaveAttribute("aria-checked", "true");
  await expect(sheet.getByRole("radio", { name: "Last hour" })).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(sheet).toHaveCount(0);
});

test("report collection keeps the dialog open until it completes", async ({ page }) => {
  await openDesktop(page);
  await page.evaluate(() => {
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = (command, args) => command === "report_collect" ? new Promise(() => {}) : invoke(command, args);
    window.__navigation.open("report");
  });
  const sheet = page.getByRole("dialog", { name: "Report a problem" });
  await sheet.getByRole("button", { name: "Collect", exact: true }).click();
  await expect(sheet.getByText("Collecting from your machines")).toBeVisible();
  await expect(sheet.getByRole("button", { name: "Cancel" })).toBeDisabled();
  await page.keyboard.press("Escape");
  await expect(sheet).toBeVisible();
});

test("a late issue preview cannot change a reopened report", async ({ page }) => {
  await openDesktop(page);
  await page.evaluate(() => {
    window.__navigation.respond("report_collect", { manifest: { version: 1, sender: "example", mesh: null, app: "desktop", app_version: "0.1", text: "", period: "today", since: 0, made_at: 0, machines: [], files: [{ path: "log.txt", bytes: 14 }] }, files: [{ path: "log.txt", text: "Log [redacted]" }] });
    window.__navigation.respond("report_send", { reference: "R-example", gh: true });
    window.__navigation.respond("report_issue_text", "Old issue text");
    const invoke = window.__TAURI__.core.invoke;
    let release: () => void = () => {};
    const gate = new Promise<void>(resolve => { release = resolve; });
    Object.defineProperty(window, "releaseReportIssue", { value: () => release(), configurable: true });
    window.__TAURI__.core.invoke = (command, args) => command === "report_issue_text" ? gate.then(() => invoke(command, args)) : invoke(command, args);
    window.__navigation.open("report");
  });
  let sheet = page.getByRole("dialog", { name: "Report a problem" });
  await sheet.getByRole("button", { name: "Collect", exact: true }).click();
  const log = sheet.getByRole("button", { name: /^log\.txt/ });
  await log.click();
  await expect(log).toBeFocused();
  await expect(log).toHaveAttribute("aria-expanded", "true");
  await expect(sheet.getByText("Read only, secrets removed")).toBeVisible();
  await expect(sheet.getByText("[redacted]", { exact: true })).toBeVisible();

  await page.evaluate(() => window.__navigation.hold("report_send"));
  const send = sheet.getByRole("button", { name: "Send", exact: true });
  await send.click();
  await expect(send).toBeDisabled();
  await expect(sheet.getByRole("button", { name: "Cancel", exact: true })).toBeDisabled();
  await page.evaluate(() => window.__navigation.release("report_send"));
  sheet = page.getByRole("dialog", { name: "Sent" });
  await expect(sheet.getByText("Writing the issue")).toBeVisible();
  await sheet.getByRole("button", { name: "No", exact: true }).click();
  await page.evaluate(() => window.__navigation.open("report"));
  sheet = page.getByRole("dialog", { name: "Report a problem" });
  await page.evaluate(() => { const release = Reflect.get(window, "releaseReportIssue"); if (typeof release === "function") release(); });
  await expect(sheet.getByRole("textbox", { name: "What happened?" })).toBeVisible();
  await expect(sheet.getByText("Old issue text")).toHaveCount(0);
});
