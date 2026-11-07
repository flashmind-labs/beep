# Mockups

Open `android/index.html`, `ios/index.html` or `desktop/index.html` in a browser.
Each entry page loads its platform's styles and scripts in order. The mockups
share state and navigation within a platform; they are not independent pages.

Find a screen with `rg` in its platform folder. Edit the relevant screen file
and shared styles only as needed. Numbered scripts preserve execution order;
keep their order in `index.html`, since later files extend earlier screens.
When adding a screen, put its code in a descriptively named file and register
it in the entry page. Keep large assets in `assets/`, not inline in source.
Do not combine screens into a single large file.

After editing, open the affected mockup and check the screen, its states and
navigation. Update the mock before changing the app UI.
