# Plugins

Prototype. A plugin is a folder `<plugins dir>/<id>/` with a `beep-plugin.json`
and TypeScript files. The desktop runs it in an embedded sandbox, reloads it
when its files change, and draws what it places. A plugin never touches the DOM.
There are three slots and three permissions (`ui.add`, `sessions.read`, `tools`).
Folders and files whose names start with a dot are never plugins.

## Installing and approval

Settings, Plugins lists every plugin with its source, permissions and state, and
turns each on or off. Install one from Git (`owner/repo`, or an https or ssh
address, with an optional branch, tag or commit and an optional folder of the
repository) or from a folder on the computer. Beep fetches it into a staging
folder, shows the plugin, its source and commit and what it asks for, and installs
only when you choose Allow and install. Cancel deletes the staging folder. A Git
plugin's Update looks for a newer commit on its branch and shows the permissions
it adds or drops before anything changes. Beep never updates a plugin on its own.
Installs are capped at 16 MB and refuse symlinks that leave the plugin folder.
Git needs to be installed; it runs with no prompts, no hooks and no submodules.

Beep keeps one record per plugin in `<plugins dir>/.plugins.json`: whether it is
on, the permissions you allowed and where it came from. A plugin runs only when
it is on and every permission its manifest asks for is allowed. A manifest that
adds a permission, by an update or an edit, puts the plugin back to Needs approval
and it stops until you allow it in Settings. A broken or missing record file is
logged and read as empty.

A folder in the app data plugins dir with no record waits for approval. When
the plugins dir is named by `BEEP_PLUGINS_DIR` (development), a plugin with no
record counts as on and allowed, and shows as a Dev folder. Turning it on or off,
installing or updating it makes a record, and the record rules apply from then on.

The composer cat ships inside the desktop app as a built-in plugin (source
Built in). At each start Beep installs it into the plugins dir, allowed for what
its manifest asks, and on unless you turned it off; that choice survives restarts
and updates. When the app's copy differs from the folder, the app's files replace
the folder, so edits to a built-in are overwritten at the next start: copy it under
a new id to change it. A built-in cannot be removed, and installing another plugin
with its id is refused. Under `BEEP_PLUGINS_DIR` nothing is written; a plugin in
that folder with the same id runs instead.

## Manifest

```json
{ "id": "composer-cat", "name": "Composer cat", "version": "0.2.0", "api": 1,
  "description": "A cat sits on the message box.", "entry": "main.ts", "permissions": ["ui.add", "sessions.read"],
  "activation": ["slot:composer"] }
```

- `id`: 2 to 40 chars, lowercase letters, digits and dashes, starts with a
  letter, equals the folder name.
- `version`: semver. `api`: the plugin API version, default 1. This Beep knows
  version 1. A plugin that asks for a newer one does not load, and Settings shows
  "needs a newer Beep". `description`: optional. `entry`: optional, default
  `main.ts`, a path inside the folder.
- `activation`: optional, when the plugin starts, default `["startup"]`. See
  Activation.
- `permissions`: `ui.add` (place UI), `sessions.read` (follow `session.turn`,
  `agents` and `agent` topics) and `tools` (give coding agents tools, see Tools for agents).
  Unknown permissions or fields fail the load.
- `tools`: optional, the tools the plugin gives coding agents. Needs the `tools`
  permission.

## API

Types are in `sdk/beep.d.ts`. There is no DOM or `setTimeout`.

```ts
/// <reference path="../../sdk/beep.d.ts" />
const frame = beep.state("idle");
beep.place({ slot: "composer", align: "end", offset: 84, sink: 4 }, () =>
  beep.image({ src: `art/cat-${frame.get()}.png`, width: 60, height: 48, pixelated: true }));
beep.place({ slot: "session.row" }, row => beep.text(row.title, { tone: "dim" }));
const cancel = beep.after(140, () => frame.set("blink"));
const stop = beep.on({ name: "ui.composer" }, event => { /* event.data */ });
```

- `place(options, render)`: `render(context)` runs once for each instance of the
  slot on the page, and returns a node, or `null` to draw nothing there. It runs
  again for an instance whose context changed, and for all of them after any
  state change. Instances that did not change are not rendered again.
- `state(initial)`: `get()` and `set(next)`.
- `after(ms, callback)`: one-shot timer, at least 16 ms, returns a cancel function.
  Timers wait while no window is visible.
- `on(topic, callback)`: follow a topic, see below. Returns a function that stops it.
- `log(...values)`, and `console.log`, `console.warn`, `console.error`: see Logs.
- `store.get(key)`, `store.set(key, value)`, `store.delete(key)`: see Store.
- `tool(name, handler)`: answer a tool coding agents call, see Tools for agents.
- `image`, `sprite`, `text`, `row`, `button`, `toggle`, `input`: build nodes. Images
  are paths inside the folder, at most 256 KB each and 4 MB per plugin.

## Files

The entry can import other `.ts` and `.js` files of the plugin folder with
relative paths: `import { draw } from "./lib/draw.ts"`. Types are stripped from each
file as it loads. Imports need the extension. Only relative paths that stay inside
the folder load (a symlink out of it is refused); there are no bare specifiers,
URLs or other file types. A plugin loads at most 32 files and 512 KB of source.
Imports that cycle work as in any ES module. `beep` and `console` are global in
every file. Top-level code in all files runs before the first render, under the
same CPU budget, and an error in any of them stops the plugin. A change to any file
in the folder reloads the plugin.

## Logs

There is one log file, the app's. `beep.log` and `console.log` write at info,
`console.warn` at warn and `console.error` at error. Values format as in
`console.log`: strings as they are, the rest as compact JSON, on one line cut at
1 KB. A plugin writes at most 20 lines a second; the rest are counted and one
`dropped N lines` line follows. Uncaught errors, budget stops and load failures are
logged too, at error. Every line has the target `plugin` and the plugin id first:

```
INFO plugin: session-notes: loaded 3 notes
```

Filter one plugin's lines with `plugin: <id>: `.

## Store

`beep.store` keeps JSON values for the plugin across restarts, with no permission.
`get` returns `null` for a missing key, `set` throws for a value that is not JSON or
when all keys and values would pass 256 KB. Changes are written to
`<plugins dir>/.data/<id>.json` a couple of seconds later (atomically) and when the
plugin stops or reloads. Disabling a plugin or updating it (a built-in too) keeps
its data; removing it deletes the data.

## Tools for agents

Coding agents reach plugins through the `plugins` tool of the Beep MCP server, on
the desktop's own node and, through the mesh, on any other machine's node (the
call goes to an online desktop; a phone runs no plugins and never answers). The
Settings page shows the permission as "Give tools to your agents": coding agents can
call the tools this plugin lists.

What an agent can do:

- `list`: every plugin with its state, source, permissions and tools.
- `status`: one plugin, with its placements count and its recent log lines (up to
  200, default 50) read from the newest daily file of the app's log, the lines
  whose message starts with `plugin: <id>: `.
- `create`: write a starter plugin (id, name, description, permissions) into the
  plugins folder as a local plugin. It does not run until the user allows it in
  Settings, Plugins. In a development folder (`BEEP_PLUGINS_DIR`) it runs at once,
  as any plugin there does.
- `call`: call a tool a plugin provides with arguments. The result comes back as
  text (JSON when it is not a string), at most 64 KB, and is untrusted output.

What an agent cannot do: allow a plugin or a permission, turn one on or off, install,
update or remove one, or call a plugin that is off, waiting for approval, paused
(no window visible) or stopped; the call fails and names the state. A plugin that
waits for its slot starts for the call.

A plugin declares each tool in its manifest and registers its handler:

```json
{ "permissions": ["tools"],
  "tools": [{ "name": "get_note", "description": "The note on a session.",
              "input": { "type": "object", "properties": { "session": { "type": "string" } },
                         "required": ["session"], "additionalProperties": false } }] }
```

```ts
beep.tool<{ session: string }>("get_note", ({ session }) => ({ note: notes.get()[session] ?? "" }));
```

- Names match `^[a-z][a-z0-9_]{0,40}$`. A plugin has at most 8 tools, each description
  is at most 300 characters, and `input` is a JSON Schema that the host compiles when
  it reads the manifest. The host checks the arguments against it before the
  handler runs.
- A handler returns a JSON value or a promise of one, and runs on the plugin's thread
  under the usual CPU budget. A thrown error is the call's error. A promise that does
  not settle within 10 seconds fails the call and the plugin keeps running.
- A tool declared but never registered, or registered with `beep.tool` but not
  declared, stops the plugin and its status says which.
- `examples/session-notes` gives `get_note` and `set_note`, so an agent reads and
  writes the notes the UI shows.

## Slots

A slot can be on the page many times, and the page tells the host each one with
its own context. `render` gets that context. A slot with no instance is not rendered.
At most 200 instances of a slot are drawn, and a plugin has up to 8 `place` calls.

| Slot | Where | Context |
| --- | --- | --- |
| `composer` | The top edge of the open session's message box. | `{ machine, session }` |
| `session.row` | A session's row in the Coding agents list, between the title and the state. | `{ machine, session, title }` |
| `session.header` | The open session's header, after the title. | `{ machine, session }` |

`composer` stands on an edge: its options are `align` (`start`, `center`, `end`,
default `end`), `offset` and `sink` in px. It takes no pointer except on controls.
The other slots flow inline and take only `slot`; other options fail the load. A
slot draws nothing, and takes no room, until a plugin fills it.

Every slot also takes `enter` and `leave`, each `"none"` (default), `"fade"` or
`"rise"`: a short (about 150 ms) transition the page does with CSS when the node
appears, and when the plugin stops drawing it. A node that left stays on the page
until its leave is done, and comes back if the plugin draws it again before then.
Both are skipped under reduced motion.

`session.row` takes no controls, since the whole row is a button. Rendering a
button, toggle or input there stops the plugin.

There is no `message` slot yet: the conversation log is windowed, with turn
heights measured by hand, and its rows have no stable message id.

## Nodes

- `text(text, { tone })`: `tone` is `"normal"`, `"dim"` or `"accent"`.
- `row({ gap }, children)`: children left to right.
- `image({ src, width, height, pixelated })`.
- `sprite({ rest, width, height, pixelated, motions, reactions, play })`: see Animation.
- `button({ label, onPress }, children?)`: `label` is the accessible name and the
  content when there are no children.
- `toggle({ label, value, onChange })`: `onChange(value)` gets the new value.
- `input({ label, value, placeholder, onChange, onSubmit })`: one line of text, up
  to 4 KB. `onChange(text)` runs on each change and `onSubmit(text)` on Enter.

Callbacks run on the plugin's thread, then the plugin draws again. Keep an input's
`value` in a state and set it in `onChange`: the page shows what is typed at once
and takes the plugin's `value` when that changes. A callback from an older draw
is ignored. A plugin gets at most 30 events a second. Controls look like the app's
own, are keyboard reachable, and take the pointer only where they are. Using
them needs only `ui.add`.

## Animation

A plugin never animates by timers: that wakes its thread, runs its script and sends
a new tree to the page for every frame. Declare the animation with `sprite` and the
page plays it, with no wakeups of the plugin and no traffic.

```ts
beep.sprite({
  rest: "art/cat-idle.png", width: 72, height: 56, pixelated: true,
  motions: [
    { steps: [{ frame: "art/cat-blink.png", ms: 140 }], repeat: { every: { min: 3000, max: 7000 } } },
  ],
});
```

The sprite shows `rest`. Each motion is a list of steps (a frame path and how long
it shows, 16 to 60000 ms), and after the last step the sprite goes back to `rest`.
`repeat` says when a motion plays:

- `"once"` (default): once, when the sprite appears.
- `{ every: { min, max } }`: after a random delay between `min` and `max` ms (100 to
  3600000), and again after each play with a new delay.
- `"loop"`: again and again with no rest. It must be the only motion.

Motions play one at a time, in due order. A sprite has at most 8 motions, 64 steps
in each and 32 distinct frames. To change what the sprite does, return another node
after a state change: the cat types (a looping motion) while its session works, and
rests on another frame while it asks. The page draws only the rest frame while its
window is hidden or the user asks for reduced motion, and starts the motions again
when it can.

A sprite can also declare reactions, named motions that play once when asked:

```ts
beep.sprite({
  rest: "art/cat-idle.png", width: 72, height: 56, pixelated: true,
  reactions: { happy: { steps: [{ frame: "art/cat-happy.png", ms: 1400 }] } },
  play: { name: "happy", id: 3 },
});
```

`play` asks for one. The page plays it once for each new `id`, over whatever plays
now, then starts the motions again. Keep the request in the node and change `id` to
ask again: the same `id` on a later draw plays nothing, and a request already in the
node when the sprite appears is not played. Up to 8 reactions, and their frames
count toward the 32. A reaction does not play while the window is hidden or under
reduced motion. The plugin sets no timer for any of it.

Images are sent to the page once per content hash, not in every update, and the page
drops one when no placement uses it.

## Activation

`activation` lists what starts the plugin, and any one is enough:

- `"startup"`: when the app starts. The plugin runs until the app quits. Default.
- `"slot:<slot>"`: when the page first shows an instance of that slot.
- `"topic:ui.<name>"`: when the page first publishes that `ui.` topic.

The host reads the manifest without running the plugin, so a plugin that waits costs
nothing. A plugin whose activation is only slots stops when none of those slots has
been on the page for 60 seconds, and starts again when one returns, with its state
reset. List every slot the plugin draws in. A plugin that lists `startup` or a topic
keeps running once started.

## Hidden windows and cost

The page tells the host when its windows are visible. While none is, plugins are
paused: nothing is drawn, timers wait (overdue ones run once each when a window shows
again), and topic updates pile up, only the newest per topic and machine kept, and
arrive then. A plugin cannot run while hidden.

The host adds up the CPU time a plugin uses in its script (time waiting for a busy
machine does not count). A plugin that uses more than
500 ms of CPU in any 60 seconds is stopped, with the reason, and shows nothing until its files
change. The `plugins_status` command lists each plugin's id, name, version, description,
`permissions` asked for, `approved` permissions, the `tools` it declares, `source` (`local`, `git`, `dev` or `builtin`),
`enabled`, `state` (`waiting`, `running`, `paused`, `disabled`, `needs-approval` with
the `missing` permissions, or `stopped` with a `reason`), `usedMs` (CPU time) over the last minute
and `pendingTimers`. The desktop emits `plugins-status` when a plugin is added, removed
or changes state.

## Topics

`beep.on({ name, key?, machine? }, callback)` calls `callback(event)` with
`event.topic`, `event.machine` (where it came from) and `event.data`. The
callback is one more entry into the sandbox, followed by the usual redraw.

- `ui.composer`: the session whose message box is shown. `data` is
  `{ machine, session }`, or `null` when none is shown. A plugin that follows it
  late gets the latest value at once. No permission needed. The `composer`
  slot's context says the same, so most plugins need not follow it.
- `session.turn` (a session id as `key` and its `machine`): where the session's turn
  stands. It needs `sessions.read` and is stable. `data` is
  `{ session, state, outcome, turn }`:
  - `state` is `starting`, `working` (also while subagents run), `asking`,
    `idle` or `ended`.
  - `outcome` is how the last turn ended, `null` before the first: `finished`,
    `cancelled`, `failed`, `refused`, `token-limit`, `request-limit` or `stopped`
    (any other reason).
  - `turn` counts the turns that ended. A bigger number than the last update is a
    new end, the same number is a repeat. The first update may describe an end that
    came before you followed, so compare against it rather than react to it.

  The app builds it from the session views it already follows (the open session,
  and ones that run or ask), so nothing more is asked of the machines, and a session
  it does not follow sends nothing. A plugin that follows late gets the latest one.
- `agents` and `agent` (a session id as `key`, or all sessions without one):
  the app's session topics. They need `sessions.read`.
- Any other name is refused.

`session.turn` and the `ui.` topics are stable. The payloads of `agents` and `agent`
are the app's internal view shapes: they can change between releases, so prefer
`session.turn`, narrow `data` and do nothing when it is not what you expect.

Updates queue per plugin and only the newest per topic and machine is delivered.
Events over 256 KB are dropped.

Limits: 16 MB memory, 50 ms of CPU per call (each delivery, event and render
pass too) and 500 ms per minute, 64 pending timers, 16 subscriptions, 8 `place` calls, 200 instances
per slot, 64 nodes and 64 KB per drawn instance. Breaking one stops the plugin.

## Try the example

The desktop loads plugins from `plugins/` in its app data folder, or from the
folder named by `BEEP_PLUGINS_DIR`, which runs plugins without approval records, as
described above. Phones run none.

```
BEEP_PLUGINS_DIR=$PWD/plugins/examples just desktop-dev
```

`examples/session-notes` puts a note on each session's row, set from an input
and a button in the open session's header, and keeps the notes in its store.
`examples/composer-cat` follows `ui.composer` and `session.turn`: it types while the
turn runs, asks while the session asks, and reacts once to each way a turn ends.

Regenerate the cat frames with
`python3 -I plugins/examples/composer-cat/art-src/sprite.py`.
