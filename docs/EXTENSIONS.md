# Writing Soundworks extensions

Extensions are small JavaScript programs that run inside Soundworks, react to what the player is doing and talk to the outside world. This page is everything you need to write one.

If you only want to see real ones, read the two examples:

- [`examples/discord-rich-presence`](../examples/discord-rich-presence): events, timers, binary I/O over a named pipe, permissions and configuration.
- [`examples/sleep-timer`](../examples/sleep-timer): a panel in the interface, bottom bar button, commands in the search box and playback control.
- [`examples/download-from-url`](../examples/download-from-url): `sw.fetch`-style networking: a panel where you paste a link, the file is downloaded and added to your library.

- [Quick start](#quick-start)
- [How an extension is laid out](#how-an-extension-is-laid-out)
- [The manifest](#the-manifest)
- [Runtime environment](#runtime-environment)
- [API reference](#api-reference)
- [Networking and downloads](#networking-and-downloads)
- [Interface: buttons, panels and commands](#interface-buttons-panels-and-commands)
- [Events](#events)
- [Permissions](#permissions)
- [Configuration and storage](#configuration-and-storage)
- [Debugging](#debugging)
- [Limits and isolation](#limits-and-isolation)
- [Example: Discord Rich Presence](#example-discord-rich-presence)
- [Example: Sleep Timer](#example-sleep-timer)
- [Example: Download from URL](#example-download-from-url)
- [Sharing your extension](#sharing-your-extension)
- [What is not available yet](#what-is-not-available-yet)

## Quick start

1. Open Soundworks, go to **Settings > App > Extensions** and press **Open extensions folder**. It is `%APPDATA%\Soundworks\plugins`.
2. Create a folder there, for example `hello`, with two files.

`soundworks.json`
```json
{
  "id": "hello",
  "name": "Hello",
  "version": "1.0.0",
  "api": 1
}
```

`main.js`
```js
sw.on("trackchange", function (track) {
  sw.toast("Now playing: " + track.title + " by " + track.artist);
});
```

3. Back in Settings press **Refresh**, then switch **Hello** on.
4. Play something. A toast appears at every track change.

To change the code, edit the file, then switch the extension off and on again (that restarts it).

## How an extension is laid out

```
%APPDATA%\Soundworks\plugins\
  my-extension\
    soundworks.json    required: describes the extension
    main.js            required: your code (the name can be changed with "main")
    config.json        optional: user overrides for "config" in the manifest
```

You can split code into more files only by bundling them into `main.js` yourself (esbuild, rollup, ...). There is no `require` or `import` of other local files.

## The manifest

`soundworks.json`:

| Field | Required | Meaning |
|---|---|---|
| `id` | yes | Unique name. Lowercase letters, digits, `-`, `.` and `_`. This is also the key used for storage and logs. |
| `name` | yes | Display name. |
| `version` | no | Your version, shown in Settings. |
| `author` | no | Shown in Settings. |
| `description` | no | One sentence, shown in Settings. |
| `api` | yes | The extension API version you wrote for. Currently `2` (version 1 plus the interface and playback features below). Soundworks refuses extensions that need a newer API than it has. |
| `main` | no | Entry file, relative to the folder. Default `main.js`. |
| `permissions` | no | List of permissions the extension needs. See [Permissions](#permissions). |
| `config` | no | Default settings. Users override them in `config.json`. |

If the manifest is invalid, the extension is listed in Settings with the reason and cannot be enabled.

## Runtime environment

Your code runs in [QuickJS](https://bellard.org/quickjs/) (ES2020), in its own thread and its own isolated runtime. Every extension is separate: one cannot see or break another.

Available:

- The JavaScript language and its standard library (`JSON`, `Math`, `Date`, `Promise`, `Map`, ...).
- `setTimeout`, `setInterval`, `clearTimeout`, `clearInterval` (minimum resolution 10 ms, checked every 40 ms).
- `console.log / info / warn / error`.
- The global `sw` object described below.

Not available: the DOM, `window`, Node.js APIs (`require`, `fs`, `process`), the global `fetch` (use `sw.fetch`), `XMLHttpRequest`, `TextEncoder`/`TextDecoder` (use `sw.utf8`), and `import`/`export`. Your file is a plain script.

## API reference

### `sw.api`, `sw.version`, `sw.pid`
`sw.api` is the API version of the host (`2`), `sw.version` the Soundworks version string, `sw.pid` the process id of Soundworks (some protocols, like Discord's, want it).

### `sw.on(event, callback)` / `sw.off(event, callback)`
Subscribe or unsubscribe. See [Events](#events). Callbacks receive a [player state](#player-state) object. A callback that throws does not stop the extension; the error is logged and shown in Settings.

### `sw.player.getState()`
Returns the current [player state](#player-state). `position` is extrapolated while playing, so you can call it any time and get a fresh value.

### `sw.player` controls (needs the `playback` permission)
`sw.player.play()`, `pause()`, `toggle()`, `next()`, `prev()`, `seek(seconds)` and `setVolume(0..1)`. They return `true` if the request was queued. Controls act asynchronously: the player state changes a moment later and you get the matching event.

### `sw.toast(text)`
Shows a short message at the bottom of the Soundworks window. At most 20 pending, 200 characters each.

### `sw.log(...)`
Writes to the extension log (see [Debugging](#debugging)). Same as `console.log`.

### `sw.now()`
Milliseconds since the Unix epoch, as a number.

### `sw.config`
The merged configuration object, see [Configuration and storage](#configuration-and-storage).

### `sw.storage.get(key, fallback)` / `sw.storage.set(key, value)`
Small persistent key-value store, private to your extension. Values are anything JSON can hold. `get` returns `fallback` if the key does not exist. `set` returns `false` if the 1 MB quota would be exceeded.

### `sw.pipe` (needs a `pipe:` permission)
Talk to a local Windows named pipe, the way many desktop apps expose a local API.

- `sw.pipe.open(name)` opens `\\.\pipe\<name>` for reading and writing. Returns a numeric handle, or `null` if the pipe does not exist, is busy or the name is not allowed by your permissions.
- `sw.pipe.write(handle, bytes)` writes an array of byte values (numbers 0..255). Returns `true` on success.
- `sw.pipe.read(handle)` returns an array of the bytes that are available right now (possibly empty), or `null` if the pipe is closed. It never blocks. Poll it from a timer.
- `sw.pipe.close(handle)`.

### `sw.utf8.encode(string)` / `sw.utf8.decode(bytes)`
Convert between strings and arrays of UTF-8 bytes. Needed for anything that goes through `sw.pipe`.

## Networking and downloads

These need permissions that name exactly what you talk to. Without them every call is refused.

### `sw.fetch(url, options)` (needs `network:<host>`)

Returns a Promise of a response, modelled on the web `fetch`:

```js
sw.fetch("https://api.example.com/search?q=" + encodeURIComponent(q), {
  method: "GET",                       // default "GET"
  headers: { Authorization: "Bearer " + token },
  body: JSON.stringify({ a: 1 }),      // string, or an array of byte values
  timeout: 20000,                      // milliseconds, default 30000
})
  .then(function (res) {
    // res.ok, res.status, res.statusText, res.url
    // res.headers.get("content-type"), res.headers.all
    return res.json();                 // or res.text(), or res.bytes() (array of byte values)
  })
  .then(function (data) { /* ... */ })
  .catch(function (err) { sw.log("failed", err.message); });
```

- HTTP error statuses (404, 500, ...) are **not** errors: the promise resolves and `res.ok` is `false`. The promise rejects only when the request could not be made (blocked by permissions, network failure, timeout, response too large).
- Responses are limited to 16 MB. For anything bigger use `sw.download`.
- Redirects are followed (up to 5), and **every** redirect target is checked against your permissions again.
- Up to 4 requests in flight per extension. A rejection nobody handles is written to the extension log.
- HTTP and HTTPS only.

### `sw.download(url, options)` (needs `download` and `network:<host>`)

Streams the response into a file in the Soundworks downloads folder (`%USERPROFILE%\Music\Soundworks`) and resolves to `{ ok, status, path, size, url }`. Options: `filename` (sanitised; path separators and special characters are replaced), `headers`, `method`, `body`, `timeout` (default 10 minutes). Existing files are never overwritten: the name becomes `song (1).flac`. A file only appears under its final name when the download is complete. Limit: 512 MB per file.

### `sw.library.add(paths)` (needs `library`)

Adds audio files to the Soundworks library. It accepts a path or an array of paths and only works for audio files **inside the downloads folder**, so combine it with `sw.download`. Returns `true` if at least one file was accepted. Soundworks then reads the tags and shows the tracks like any other.

```js
sw.download(url, { filename: "song.flac" }).then(function (r) { sw.library.add(r.path); });
```

### Which hosts a `network:` permission allows

| Permission | Allows |
|---|---|
| `network:api.example.com` | exactly that host (any port, http or https) |
| `network:*.example.com` | any subdomain of `example.com`, not `example.com` itself |
| `network:*` | any public host |

Wildcards never match IP addresses, `localhost` or `.local` / `.internal` names, so an extension cannot be tricked into probing your home network. If you really need a local server, name it exactly: `network:192.168.1.20` or `network:localhost`. Prefer narrow permissions: users see them in Settings, and `network:*` is shown as "can connect to any website".

## Interface: buttons, panels and commands

These need the `ui` permission. Everything is declarative: you describe what you want, Soundworks draws it with its own look, so extensions always match the app and cannot break the layout. When your extension is switched off, everything it added disappears.

### Buttons

```js
sw.ui.addButton({
  id: "reload",            // unique inside your extension
  icon: "settings",        // see the icon list below
  tooltip: "Do the thing",
  onClick: function () { sw.toast("clicked"); },
});
sw.ui.removeButton("reload");
```
Buttons appear at the right end of the bottom bar. At most 8 per extension.

### Panels

A panel opens in the right sidebar (like lyrics and the queue) and gets its own toggle button in the bottom bar.

```js
sw.ui.addPanel({
  id: "main",
  title: "My panel",
  icon: "list",
  content: [ /* nodes */ ],
  onOpen:  function () { /* panel was opened */ },
  onClose: function () { /* panel was closed */ },
  onAction: function (action) {
    // action = { type: "click" | "input" | "switch" | "select", id: "...", value: ... }
  },
});
sw.ui.setContent("main", [ /* new nodes */ ]);   // redraws the panel
sw.ui.removePanel("main");
```

Content is a list of nodes. To change what is shown, call `setContent` again with the full new list. At most 4 panels, 300 nodes per panel.

| Node | Fields | Action sent to `onAction` |
|---|---|---|
| `{ type: "text" }` | `text`, `style`: `"title"`, `"heading"`, `"dim"` or omitted | none |
| `{ type: "button" }` | `id`, `label`, `primary` (bool) | `click`, `id` |
| `{ type: "list" }` | `id`, `items`: `[{ id, title, subtitle }]` | `select`, `id` = the list id, `value` = the item id |
| `{ type: "input" }` | `id`, `value`, `placeholder` | `input` when the user leaves the field or presses Enter, `value` = the text |
| `{ type: "switch" }` | `id`, `label`, `value` (bool) | `switch`, `value` = new state |
| `{ type: "divider" }` | | none |
| `{ type: "space" }` | | none |

Unknown node types are ignored, so an extension written for a newer Soundworks still opens in an older one.

### Commands

```js
sw.commands.register({ id: "next-album", title: "Skip to the next album", run: function () { ... } });
sw.commands.unregister("next-album");
```
Commands show up in the search box (F8): type `>` to list all of them, or any text that matches the title. At most 50 per extension.

### Icons

`play pause prev next repeat shuffle volume close trash power list import undo save folder spectrum spectrogram scope wave search user track upload lyrics settings headphones maximize plus`. An unknown name shows `plus`.

## Events

Subscribe with `sw.on("name", function (state) { ... })`.

| Event | When |
|---|---|
| `trackchange` | A different track started, or the extension was just enabled while something is loaded. Treat it as "full refresh". |
| `play` | Playback started or resumed on the same track. |
| `pause` | Playback was paused. |
| `seek` | The position jumped (user seeked). |
| `stop` | The extension is being shut down (disabled, or Soundworks is closing). You have a moment to clean up. |

Events are delivered one at a time, in order. Position updates while playing are not events; compute them from `getState()` when you need them.

### Player state

```js
{
  title: "Song",
  artist: "Band",
  album: "Album",
  year: "2015",        // empty string if the file has no valid year tag
  volume: 0.8,         // 0..1
  duration: 200.5,     // seconds, 0 if unknown
  position: 12.3,      // seconds
  playing: true
}
```

## Permissions

Extensions can compute and react to the player freely. Anything that reaches outside the sandbox needs a permission declared in the manifest. Permissions are shown to the user in Settings next to the extension, and a call that is not covered is refused (the call fails and the reason is logged).

| Permission | Allows |
|---|---|
| `pipe:<name>` | Opening the named pipe `<name>`. A trailing `*` matches a prefix: `pipe:discord-ipc-*`. Names may not contain `\`, `/` or `..`. |
| `network:<host>` | `sw.fetch` and `sw.download` to that host. `*` and `*.domain` wildcards, see [above](#which-hosts-a-network-permission-allows). |
| `download` | `sw.download`: writing files into the downloads folder. |
| `library` | `sw.library.add`: adding downloaded audio files to the library. |
| `ui` | Adding buttons, panels and commands to the interface. |
| `playback` | Controlling playback: play, pause, next, previous, seek and volume. |

Ask for as little as you can. Users decide whether to trust you from this list.

## Configuration and storage

Put defaults in the manifest:

```json
"config": { "greeting": "hi", "volumeStep": 5 }
```

Users change them by creating `config.json` next to the manifest:

```json
{ "greeting": "hello" }
```

Keys in `config.json` replace the same keys from the manifest (shallow merge). The result is `sw.config`. The configuration is read when the extension starts.

`sw.storage` is for data your code writes itself (tokens, counters, caches). It lives in `%APPDATA%\Soundworks\plugin-data\<id>.json` and survives updates of the extension folder.

## Debugging

- Errors thrown by your callbacks and timers are caught and shown under the extension in **Settings > App > Extensions**.
- `sw.log` and `console.log` write to `%APPDATA%\Soundworks\plugin-data\<id>.log` (at 256 KB the file is renamed to `<id>.log.old` and a new one starts). Keep a terminal tailing it while you develop:
  ```powershell
  Get-Content "$env:APPDATA\Soundworks\plugin-data\hello.log" -Wait
  ```
- Switch the extension off and on to restart it with your latest code.

## Limits and isolation

- Each call into your code (script start, an event, a timer) may run for at most **2 seconds**. After that it is interrupted with an error and the extension keeps running. An infinite loop cannot freeze Soundworks.
- Memory per extension is capped at 64 MB and the stack at 512 KB.
- Storage per extension is capped at 1 MB.
- An error or crash in one extension never affects Soundworks or other extensions.

## Example: Discord Rich Presence

[`examples/discord-rich-presence`](../examples/discord-rich-presence) shows your current track on your Discord profile as "Listening to Soundworks".

How it works: Discord listens on a local named pipe (`\\.\pipe\discord-ipc-0` to `-9`). A client opens it, sends a handshake with an application id, then sends `SET_ACTIVITY` messages. Every message is a frame: 4 bytes opcode, 4 bytes length (both little endian), then JSON. The extension implements exactly that on top of `sw.pipe`, so it also doubles as a reference for talking to any pipe-based local API.

To use it:

1. Go to <https://discord.com/developers/applications>, create an application and name it as you want it to appear ("Soundworks").
2. Copy its **Application ID**.
3. Optional: under **Rich Presence > Art Assets** upload an image named `soundworks` to show a big picture.
4. Copy the folder `examples/discord-rich-presence` into `%APPDATA%\Soundworks\plugins\`.
5. Create `config.json` in that folder:
   ```json
   { "clientId": "YOUR_APPLICATION_ID" }
   ```
6. Settings > App > Extensions > Refresh, then switch it on. Discord must be running (the desktop app, not the browser).

Other settings in `config.json`: `largeImage` (art asset name, default `soundworks`), `showWhenPaused` (default `false`: the status is cleared when you pause).

## Example: Sleep Timer

[`examples/sleep-timer`](../examples/sleep-timer) pauses playback after a chosen time and fades the volume out at the end. It uses a panel with buttons and a switch, four commands in the search box, `sw.storage` to remember the switch, and `sw.player` to control the volume and pause. Copy the folder into `%APPDATA%\Soundworks\plugins\` like any other extension. Its `config.json` can change the presets (`"presets": [10, 20, 90]`) and the fade length (`"fadeSeconds": 30`).

## Example: Download from URL

[`examples/download-from-url`](../examples/download-from-url) adds a panel with a text field and a button. Paste a direct link to an audio file, press **Download**, and the file is saved and appears in your library. It shows `sw.download`, `sw.library.add`, promises with `.catch`, and a panel whose content is rebuilt as the state changes. It asks for `network:*` because it must reach whichever host you paste; an extension for one service should ask for that service only.

## Sharing your extension

- Put the extension folder at the root of a GitHub repository (or in a subfolder), with a short README that tells people what permissions it needs and why.
- Add the topic **`soundworks-extension`** to the repository so others can find it.
- Tag releases and attach a `.zip` of the folder. Users unzip it into `%APPDATA%\Soundworks\plugins\`.
- Bump `version` in the manifest with every release.

## What is not available yet

The first API is deliberately small. These are planned, and your feedback decides the order:

- Menu items (for albums and tracks), settings pages and keyboard shortcuts.
- Custom drawing: a canvas for visualisations and whole new views.
- Providing a new music source that plays straight from a stream (for example a Subsonic server) without downloading first.
- Themes and replacing whole views.

If you need one of these, open an issue and describe what you want to build. Real use cases shape the API far better than guesses.
