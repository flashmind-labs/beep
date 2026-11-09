# Mockups

Open `android/index.html`, `ios/index.html` or `desktop/index.html` in a browser.
Desktop entry pages load styles and scripts in order and share state and
navigation. Phone entry pages are galleries of
standalone screens in each platform's `screens/` folder.

Find a screen with `rg` in its platform folder. Edit the relevant screen file
and shared styles only as needed. Numbered scripts preserve execution order;
keep their order in `index.html`, since later files extend earlier screens.
When adding a screen, put its code in a descriptively named file and register
it in the entry page. Keep large assets in `assets/`, not inline in source.
Do not combine screens into a single large file.

After editing, open the affected mockup and check the screen, its states and
navigation. Update the mock before changing the app UI.

Desktop feature modules live in `agents/`, `settings/` and `workspace/`; common
styles live in `shared/`. Prefer those files when changing an existing feature.
Phone styles live in `ios/styles/` and are shared with Android. Keep iframe
titles and screen links in sync when adding phone screens. See README.md for
the layout and `npm run check` browser validation.
