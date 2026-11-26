import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const responses = {
  settings: { name: "build-box", version: "0.1", autostart: false, agent_notices: false },
  list_machines: [], list_credentials: [], list_github_accounts: [], cli_logins: { github: { state: "missing" }, forgejo: [] },
};
async function credentials(page: import("@playwright/test").Page) {
  await openDesktop(page, false, responses);
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("button", { name: "Credentials", exact: true }).click();
  await settings.getByRole("button", { name: "+ Add a Forgejo/Gitea server" }).click();
  return settings;
}
test("credentials draft survives settings redraw and saves via Enter", async ({ page }) => {
  const settings = await credentials(page);
  const url = settings.getByLabel("Server URL");
  await url.fill("https://git.example.test");
  await url.evaluate(el => { if (el instanceof HTMLInputElement) { el.setSelectionRange(2, 5); Reflect.set(window, "credInput", el); Reflect.set(window, "credHost", el.closest("#set-creds")); } });
  // Coming back to the window redraws Settings while no token is typed.
  await page.evaluate(() => window.dispatchEvent(new Event("focus")));
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "settings").length)).toBeGreaterThan(1);
  await expect(url).toHaveValue("https://git.example.test");
  expect(await url.evaluate(el => el === Reflect.get(window, "credInput") && document.activeElement === el && el instanceof HTMLInputElement && el.selectionStart === 2 && el.selectionEnd === 5)).toBe(true);
  const secret = settings.getByLabel("Token");
  await secret.fill("example-secret");
  await secret.press("Enter");
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "save_credential" && JSON.stringify(call.args.credential) === JSON.stringify({ kind: "forgejo", url: "https://git.example.test", secret: "example-secret" })))).toBe(true);
  await page.keyboard.press("Escape");
  await expect.poll(() => page.evaluate(() => { const host: unknown = Reflect.get(window, "credHost"); return host instanceof HTMLElement ? host.isConnected : null; })).toBe(false);
});

test("late credential save does not clear a newer draft", async ({ page }) => {
  const settings = await credentials(page);
  await page.evaluate(() => window.__navigation.hold("save_credential"));
  await settings.getByLabel("Server URL").fill("https://git.example.test");
  await settings.getByLabel("Token").fill("first-secret");
  await settings.getByLabel("Token").press("Enter");
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "save_credential"))).toBe(true);
  await settings.getByRole("button", { name: "Cancel", exact: true }).click();
  await settings.getByRole("button", { name: "+ Add a Forgejo/Gitea server" }).click();
  await settings.getByLabel("Token").fill("new-secret");
  await page.evaluate(() => window.__navigation.release("save_credential"));
  await expect(settings.getByRole("button", { name: "Check and save" })).toBeEnabled();
  await expect(settings.getByLabel("Token")).toHaveValue("new-secret");
});

test("unreadable GitHub credentials offer the local sign-in", async ({ page }) => {
  await openDesktop(page, false, { ...responses,
    list_github_accounts: [{ id: "github-one", login: "sample", orgs: [], default: true, last4: "1234", created_at: 0, unreadable: true }],
    cli_logins: { github: { state: "signed_in", account: "sample" }, forgejo: [] },
  });
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("button", { name: "Credentials", exact: true }).click();
  await expect(settings.getByText("Re-enter this token")).toBeVisible();
  await expect(settings.getByText("…1234")).toHaveCount(0);
  await settings.getByRole("button", { name: "Use gh sign-in" }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "use_cli_login" && JSON.stringify(call.args.host) === JSON.stringify({ kind: "github" })))).toBe(true);
});

test("a failed account action can be retried", async ({ page }) => {
  await openDesktop(page, false, { ...responses,
    list_github_accounts: [{ id: "github-one", login: "sample", default: false, last4: "1234", created_at: 0, unreadable: false }],
  });
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("button", { name: "Credentials", exact: true }).click();
  await page.evaluate(() => window.__navigation.rejectOnce("set_default_github_account"));
  const makeDefault = settings.getByRole("button", { name: "Make default" });
  await makeDefault.click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "set_default_github_account").length)).toBe(1);
  await expect(makeDefault).toBeEnabled();
  await makeDefault.click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "set_default_github_account").length)).toBe(2);
  await page.evaluate(() => window.__navigation.rejectOnce("remove_credential"));
  const remove = settings.getByRole("button", { name: "Remove @sample" });
  await remove.click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "remove_credential").length)).toBe(1);
  await expect(remove).toBeEnabled();
  await remove.click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "remove_credential").length)).toBe(2);
});

test("an API key is saved for the picked provider", async ({ page }) => {
  await openDesktop(page, false, { ...responses,
    api_providers: [{ provider: "openrouter", name: "OpenRouter", env_var: "OPENROUTER_API_KEY" }, { provider: "meshy", name: "Meshy", env_var: "MESHY_API_KEY" }, { provider: "elevenlabs", name: "ElevenLabs", env_var: "ELEVENLABS_API_KEY" }],
    list_credentials: [{ id: "key.1.meshy", kind: "api_key", provider: "meshy", last4: "9f0c", account: null, env_var: "MESHY_API_KEY", created_at: 0, unreadable: false }],
  });
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("button", { name: "Credentials", exact: true }).click();
  await expect(settings.getByText("…9f0c")).toBeVisible();
  await expect(settings.getByText("MESHY_API_KEY")).toBeVisible();
  await settings.getByRole("button", { name: "+ Add an API key" }).click();
  await settings.getByRole("button", { name: "Provider OpenRouter" }).click();
  await expect(page.getByRole("menuitemradio")).toHaveText(["OpenRouter", "ElevenLabs"]);
  await page.keyboard.press("Escape");
  await expect(page.getByRole("menu")).toHaveCount(0);
  await expect(settings).toBeVisible();
  await settings.getByRole("button", { name: "Provider OpenRouter" }).click();
  await page.getByRole("menuitemradio", { name: "ElevenLabs" }).click();
  await settings.getByLabel("Key", { exact: true }).fill("el-example");
  if (process.env.SHOT) await page.screenshot({ path: process.env.SHOT });
  await settings.getByRole("button", { name: "Save", exact: true }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "save_credential" && JSON.stringify(call.args.credential) === JSON.stringify({ kind: "api_key", provider: "elevenlabs", secret: "el-example" })))).toBe(true);
});
