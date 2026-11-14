# Make It Yours

MemoryCore Castle is MIT licensed: copy it, change it, rename it, share it. This guide shows the usual changes. Castle is a [Tauri 2](https://tauri.app/) app: a Vue 3 page (`src/`) on top of a small Rust core (`src-tauri/`).

## 1. A theme of your own

The easiest way: open **THEME > Create theme**, give it a name and a picture, and Castle makes it for you.

By hand, a theme is a folder with a `theme.json` and a wallpaper:

```
%APPDATA%\com.memorycore.castle\themes\sakura-night\
  theme.json
  wallpaper.png
```

```json
{
  "id": "sakura-night",
  "name": "Sakura Night",
  "description": "soft pink blossoms under a dark blue night",
  "wallpaper": "wallpaper.png",
  "wallpaperPosition": "center",
  "widgetStyle": "glass",
  "colors": {
    "--background": "#0b0a1f",
    "--text": "#fdf0f6",
    "--text-dim": "#c6a9bb",
    "--accent": "#ff9ccf",
    "--line": "rgba(255, 170, 210, 0.34)",
    "--panel-background": "rgba(24, 14, 40, 0.62)",
    "--widget-background": "rgba(30, 18, 48, 0.55)",
    "--bar-background": "rgba(12, 8, 26, 0.72)",
    "--hover": "rgba(255, 156, 207, 0.14)",
    "--glass-blur": "12px"
  },
  "panel": { "left": "1.6%", "top": "3%", "bottom": "3%", "width": "31%" },
  "widgets": {
    "clock": { "right": "21%", "top": "4%", "size": "l" },
    "gauges": { "right": "2%", "top": "4%" },
    "usage": { "right": "31%", "bottom": "calc(4% + 150px)", "width": "33%" },
    "today": { "right": "2%", "top": "22%", "width": "28%" },
    "projects": { "right": "2%", "top": "calc(22% + 190px)", "width": "28%" },
    "diary": { "right": "2%", "top": "calc(22% + 395px)", "width": "28%" },
    "growth": { "right": "2%", "bottom": "4%", "width": "28%" },
    "backup": { "right": "31%", "bottom": "4%", "width": "16%" }
  }
}
```

Rules the loader checks (`src/themes/loader.js`): an `id` and a `name`; colours only as `--variables` in `colors`; widget places only `left`, `top`, `right`, `bottom`, `width`, `height` (and `size` for the clock: `s`, `m` or `l`). A widget your theme leaves out is not shown (the USAGE card is only on HOME when the theme gives it a place). A broken theme is listed with its reason and never shown. Press **Reload themes** after editing.

To ship a theme **inside** your build, put its folder in `src/themes/<id>/` instead - built-in themes are found automatically.

## 2. A room of your own

1. Make the view: `src/views/MyRoomView.vue`.
2. Add it to the tab list in `src/App.vue` (`allTabs`), to the `BUILT` set, and render it:
   ```vue
   <MyRoomView v-if="active === 'myroom'" />
   ```
3. If it needs the computer (files, programs), add a Rust command in `src-tauri/src/` (see `memory.rs` for reading the memory folder safely with `files::fenced`), register it in `src-tauri/src/lib.rs` (`generate_handler!`), and call it from `src/api/castle.js`.
4. A room that needs a MemoryCore feature can hide until it is installed: give its tab `needs: '<feature>'` and add the check to `src-tauri/src/memory.rs` (`features`).

## 2b. A world of your own

Members can create worlds from the WORLD room (see the user guide). To ship a 3D world **inside** your build:

1. Make `src/worlds/<id>/world.json`:
   ```json
   {
     "slug": "my-world", "title": "My World", "line": "One sentence for its card",
     "layout": "centrepiece", "camera": { "default": "orbit", "scale": 3 }, "glow": "#8fe6ff",
     "pieces": [
       { "file": "01-base.glb", "role": "base", "size": 40 },
       { "file": "02-heart.glb", "role": "companion", "size": 9 },
       { "file": "03-islet.glb", "role": "islet", "size": 8, "orbit": { "radius": 40, "height": 2, "lap": 260, "phase": 0 } }
     ]
   }
   ```
   Roles: `base` (the great isle, its top at the world's floor), `companion` (stands at the heart and glows), `islet` (circles on its `orbit`), `isle` (an archipelago island standing in for one of the built-in isles: `"isle": "heart" | "north" | "east" | "west" | "south"`) and `prop` (at a fixed `at`). `size` is the piece's largest side in metres - pieces are sized by their bounding box, so any export scale works.
2. Put your GLB exports, named by number, in a folder and run `npm run world:lighten -- <id> <folder>` (default folder: `Project Resources/Design/<id>/hd`). The prepared pieces land in `public/worlds/<id>/`; add a `preview.jpg` there for the card. To tune sizes, orbits and the camera by eye, run `npm run vite` and open `http://localhost:5173/?world=3d/<id>&camera=orbit` (dev builds only). A world's `camera` may also set its own `"orbit": { "radius", "height" }` and `"wide": { "radius", "height" }` in metres.
3. The card turns *ready* when every piece is there. Anything you add here ships with your build, so use models you have the rights to share (Tripo's free plan publishes under CC BY 4.0; a paid plan owns its output).

## 3. Teach the companion something

- Everything your companion is told about Castle lives in `src-tauri/resources/castle-block.md` (written into the memory's `CLAUDE.md` and `AGENTS.md` between `castle` markers).
- The STORY rules: `src-tauri/resources/story-rules.md`. The chat room rules: `src-tauri/resources/room-rules.md`.
  Its `{{MANNER}}` line is where the companion's emotion engine goes (only its active traits' one-line manner, by audience, written by `roomturn::manner_section`); the proposals' rules are `src-tauri/resources/emotion-rules.md`.
- The drawing styles: `src-tauri/resources/styles/*.md`.
- The media helpers: `src-tauri/resources/tools/castle-*.ps1` (run `test-helpers.ps1` after a change).

## 4. Rename it

- `package.json` (`name`), `src-tauri/Cargo.toml` (`name`), `src-tauri/tauri.conf.json` (`productName`, `identifier`, window `title`).
- Changing the `identifier` gives your build its own settings folder and its own keys in Credential Manager (`src-tauri/src/key.rs`, `SERVICE`).
- The icon: `npx tauri icon your-icon.png`.

## 5. Build your installer

```bash
npm install
npm run tauri build
```

The installer lands in `src-tauri/target/release/bundle/nsis/`.

**Updates:** only official releases update themselves. A build you make yourself never does, so it can never overwrite your changes. To bring in new Castle features, pull the upstream repository with git and merge them into your copy:

```bash
git remote add upstream https://github.com/Kiyoraka/Project-MemoryCore-Castle.git
git pull upstream main
```

## 6. Tests

```bash
npm test                      # web tests (vitest)
cd src-tauri && cargo test    # core tests (uses the fixture memories in src-tauri/tests/fixtures)
powershell -File src-tauri/resources/tools/test-helpers.ps1   # media helpers, offline
```

Two fixture memories keep Castle honest: `memory-fresh` (a new MemoryCore) and `memory-existing` (a member who already set things up). Check new features against both.
