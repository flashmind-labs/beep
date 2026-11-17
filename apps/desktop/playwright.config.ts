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
      // Repeat shortcuts, terminal input, folder actions and platform settings.
      testMatch: [
        "**/agent-mention-react.spec.ts",
        "**/agent-terminal-react.spec.ts",
        "**/agentpicker-react.spec.ts",
        "**/bar-waiting.spec.ts",
        "**/close-to-project.spec.ts",
        "**/files-tree-react.spec.ts",
        "**/find.spec.ts",
        "**/home-assistant.spec.ts",
        "**/import-project-react.spec.ts",
        "**/navigation.spec.ts",
        "**/notices-react.spec.ts",
        "**/plugins-settings.spec.ts",
        "**/project-composer-react.spec.ts",
        "**/project-window-links.spec.ts",
        "**/projects-search.spec.ts",
        "**/settings-bar-root.spec.ts",
        "**/settings-local-react.spec.ts",
        "**/shortcuts-react.spec.ts",
        "**/sidebar-shell-react.spec.ts",
        "**/terminal.spec.ts",
        "**/terminals-navigation-react.spec.ts",
        "**/view-memory.spec.ts",
        "**/zoom-focus-chat.spec.ts",
      ],
      use: { userAgent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/145.0.0.0 Safari/537.36" },
    },
    // Input and selection in WebKit, the engine of the app on macOS, iOS and Linux.
    { name: "WebKit", testMatch: /(?:image-attachments|project-window-links|terminal|selection-feedback|device-react|session-model-scroll|session-header|compose-pills)\.spec\.ts/, use: { browserName: "webkit" } },
  ],
});
