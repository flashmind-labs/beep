# Plugins

Prototype. A plugin is a folder `<plugins dir>/<id>/` with a `beep-plugin.json`
and one TypeScript file. The desktop runs it in an embedded sandbox, reloads it
when its files change, and draws what it places. A plugin never touches the DOM.
There is one slot (`composer`) and two permissions (`ui.add`, `sessions.read`).

## Manifest

```json
{ "id": "composer-cat", "name": "Composer cat", "version": "0.1.0",
  "description": "A cat sits on the message box.", "entry": "main.ts", "permissions": ["ui.add"] }
```

- `id`: 2 to 40 chars, lowercase letters, digits and dashes, starts with a
  letter, equals the folder name.
- `version`: semver. `description`: optional. `entry`: optional, default
  `main.ts`, a path inside the folder.
- `permissions`: `ui.add` (place UI) and `sessions.read` (follow `agents` and
  `agent` topics). Unknown permissions or fields fail the load.

## API

Types are in `sdk/beep.d.ts`. The source is one file with no imports, and there
is no DOM, `setTimeout` or `console`.

```ts
/// <reference path="../../sdk/beep.d.ts" />
const frame = beep.state("idle");
beep.place({ slot: "composer", align: "end", offset: 84, sink: 4 }, () =>
  beep.image({ src: `art/cat-${frame.get()}.png`, width: 60, height: 48, pixelated: true }));
const cancel = beep.after(140, () => frame.set("blink"));
const stop = beep.on({ name: "ui.composer" }, event => { /* event.data */ });
```

- `place(options, render)`: `render` runs again after a state change.
- `state(initial)`: `get()` and `set(next)`.
- `after(ms, callback)`: one-shot timer, at least 16 ms, returns a cancel function.
- `on(topic, callback)`: follow a topic, see below. Returns a function that stops it.
- `image`, `text`, `row`: build nodes. Images are paths inside the folder, at
  most 256 KB.

## Topics

`beep.on({ name, key?, machine? }, callback)` calls `callback(event)` with
`event.topic`, `event.machine` (where it came from) and `event.data`. The
callback is one more entry into the sandbox, followed by the usual redraw.

- `ui.composer`: the session whose message box is shown. `data` is
  `{ machine, session }`, or `null` when none is shown. A plugin that follows it
  late gets the latest value at once. No permission needed.
- `agents` and `agent` (a session id as `key`, or all sessions without one):
  the app's session topics. They need `sessions.read`.
- Any other name is refused.

Mesh topic payloads are the app's internal view shapes. They can change between
releases, so narrow `data` and do nothing when it is not what you expect.

Updates queue per plugin and only the newest per topic and machine is delivered.
Events over 256 KB are dropped.

Limits: 16 MB memory, 50 ms of CPU per call (each delivery too), 64 pending
timers, 16 subscriptions, 8 placements, 64 nodes. Breaking one stops the plugin.

## Try the example

The desktop loads plugins from `plugins/` in its app data folder, or from the
folder named by `BEEP_PLUGINS_DIR`. Phones run none.

```
BEEP_PLUGINS_DIR=$PWD/plugins/examples just desktop-dev
```

Regenerate the cat frames with
`python3 -I plugins/examples/composer-cat/art-src/sprite.py`.
