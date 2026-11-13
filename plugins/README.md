# Plugins

Prototype. A plugin is a folder `<plugins dir>/<id>/` with a `beep-plugin.json`
and one TypeScript file. The desktop runs it in an embedded sandbox, reloads it
when its files change, and draws what it places. A plugin never touches the DOM.
There is one slot (`composer`) and one permission (`ui.add`).

## Manifest

```json
{ "id": "composer-cat", "name": "Composer cat", "version": "0.1.0",
  "description": "A cat sits on the message box.", "entry": "main.ts", "permissions": ["ui.add"] }
```

- `id`: 2 to 40 chars, lowercase letters, digits and dashes, starts with a
  letter, equals the folder name.
- `version`: semver. `description`: optional. `entry`: optional, default
  `main.ts`, a path inside the folder.
- `permissions`: only `ui.add`. Unknown permissions or fields fail the load.

## API

Types are in `sdk/beep.d.ts`. The source is one file with no imports, and there
is no DOM, `setTimeout` or `console`.

```ts
/// <reference path="../../sdk/beep.d.ts" />
const frame = beep.state("idle");
beep.place({ slot: "composer", align: "end", offset: 84, sink: 4 }, () =>
  beep.image({ src: `art/cat-${frame.get()}.png`, width: 60, height: 48, pixelated: true }));
const cancel = beep.after(140, () => frame.set("blink"));
```

- `place(options, render)`: `render` runs again after a state change.
- `state(initial)`: `get()` and `set(next)`.
- `after(ms, callback)`: one-shot timer, at least 16 ms, returns a cancel function.
- `image`, `text`, `row`: build nodes. Images are paths inside the folder, at
  most 256 KB.

Limits: 16 MB memory, 50 ms of CPU per call, 64 pending timers, 8 placements,
64 nodes. Breaking one stops the plugin.

## Try the example

The desktop loads plugins from `plugins/` in its app data folder, or from the
folder named by `BEEP_PLUGINS_DIR`. Phones run none.

```
BEEP_PLUGINS_DIR=$PWD/plugins/examples just desktop-dev
```

Regenerate the cat frames with
`python3 -I plugins/examples/composer-cat/art-src/sprite.py`.
