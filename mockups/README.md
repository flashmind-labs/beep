# Platform mockups

Open `desktop/index.html`, `ios/index.html` or `android/index.html` directly in a browser. No build is needed. `serve.py --bind ADDRESS` adds automatic reload when serving them.

The app implementation is the reference for current screens. Keep proposed screens and server previews clearly identified in the gallery.

The desktop menu groups screens by feature in collapsible sections. Search matches screen names, IDs and section names. Enter opens the first result; Escape clears search. Section expansion is saved locally.

## Editing

- Desktop: `agents/` holds the start form, conversation, files and session list; `settings/` holds accounts, credentials and navigation; `workspace/` holds Home, projects and appearance; `shared/` holds the gallery and common styles. `current/` selects the initial screen and groups current app screens ahead of proposals.
- Existing numbered desktop files contain other screens and later extensions. Search by screen ID or visible label. Their order in `desktop/index.html` matters: later scripts extend earlier renderers. Keep stylesheet placement when splitting files so the cascade stays unchanged.
- Phones: each platform's `screens/` contains named standalone HTML screens. iOS `styles/` contains the shared phone styles; Android loads those styles and applies its platform rules. Entry pages arrange the screens in a gallery using iframes, which also work from disk.
- Put reusable artwork in `assets/`. Keep sample machines, paths and projects fictional.

Home and a project open on the board (`workspace/board.js`), which reads its cards from the list layout's rows; Settings, Appearance switches to the list. Compare the board with `apps/desktop/src/center/lanes-view.tsx`, Home with `apps/desktop/src/center/home.ts`, machine columns with `src/machines/table.ts`, settings with `src/settings/`, and phone sizing with `apps/desktop/dist/mobile.css`.

## Browser checks

From this folder, run `npm ci`, then `npx playwright install chromium` once. Run `npm run check` after changes. It opens every registered desktop screen and variant, checks both phone galleries and saves review screenshots in the ignored `test-results/` folder.
