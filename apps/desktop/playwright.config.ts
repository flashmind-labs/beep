import { defineConfig } from "@playwright/test";

const executablePath = process.env.CHROMIUM_BIN;

export default defineConfig({
  testDir: "./tests/browser",
  fullyParallel: true,
  forbidOnly: Boolean(process.env.CI),
  retries: 0,
  // Screenshots are never committed: a machine records its own on the first run.
  updateSnapshots: "missing",
  use: {
    browserName: "chromium",
    viewport: { width: 1280, height: 900 },
    launchOptions: executablePath ? { executablePath } : {},
    trace: "retain-on-failure",
  },
  projects: [
    { name: "macOS", use: { userAgent: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 Chrome/145.0.0.0 Safari/537.36" } },
    {
      name: "Windows",
      // Repeat flows that exercise Windows-specific shortcuts or controls.
      grep: /@windows/,
      use: { userAgent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/145.0.0.0 Safari/537.36" },
    },
    // Input and selection in WebKit, the engine of the app on macOS, iOS and Linux.
    { name: "WebKit", testMatch: /(?:file-editor|assistant-rail|image-attachments|project-rail|project-window-links|terminal|selection-feedback|machine-mcp-servers|device-react|session-model-scroll|big-log|session-header|compose-pills|usage-react|zoom-focus-chat)\.spec\.ts/, use: { browserName: "webkit" } },
  ],
});
