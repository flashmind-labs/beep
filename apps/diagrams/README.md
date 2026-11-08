# Conversation diagrams

Desktop, Android and iOS bundle `assets/` and render fenced `mermaid` blocks
locally. The source stays in the message. SVG and PNG exports need no server.

Copy image puts a PNG on the system clipboard. Cmd+C (Ctrl+C on Windows) copies
the visible diagram; selected source text keeps normal text copying. Right-click
an image to copy it or save it (to Downloads on desktop).

`mermaid.min.js` is built from the pinned Mermaid package. After updating its
version and lockfile, regenerate it and the bundled licenses:

```sh
npm ci
npm run build
```

Browser checks cover rendering, source fallback, exports and desktop streaming:

```sh
npx playwright install chromium
npm test
```

Set `DIAGRAM_ENGINE=webkit` to test WebKit. `DIAGRAM_BROWSER` optionally selects
an installed browser executable.
