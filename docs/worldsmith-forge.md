# The Worldsmith Forge - the tool standard

WORLDSMITH is MemoryCore Castle's game modding room, for **your own offline single-player games**. Everything it can do is a **tool**, and every tool follows this standard, so a new one plugs in without touching the others. The Alice Keep builds the same room against the same standard, and contributors (Castle is MIT) build tools this way too.

## The three kinds

Each tool is one kind. Members see the smithy name; developers see the plain word beside it.

| Kind | Plain word | What it may do | The rules that come with the kind |
|---|---|---|---|
| **Hammer** | live | change a running game through the helper | offline single-player only; the helper refuses anti-cheat and online games at attach; user mode only (no driver, no injection); closing the room, switching it off or the game ending lets go and stops every held value |
| **Lens** | screen | look at the game's picture and draw on top of it | never touches the game's memory or files; screenshots stay on the computer except what the member's own AI reads; the overlay never takes a click |
| **Chisel** | files | change the game's files on disk | a backup before any change; one press undoes it; only inside the game's own folder (not built yet - the mod manager will be the first) |

The rules belong to the kind, not to the tool: a tool cannot opt out of them. A Hammer only reaches a game through the helper's verbs, which do the refusing; a Lens has no path to write anything; a Chisel will only write through the room's own backup-first file door.

## A tool's manifest

A tool is declared once in `src/views/worldsmith/forge.js` (`TOOLS`):

| Field | Meaning |
|---|---|
| `id` | a short id (`values`, `translate`) - also the mode the Rust side is told |
| `name` | what the chooser shows |
| `kind` | `hammer`, `lens` or `chisel` |
| `line` | one plain line in the chooser |
| `icon` | the chooser card's drawing |
| `needs` | the helper features it requires (see Capabilities); a tool whose needs are missing shows "Needs a newer helper" and never breaks |

Beside the manifest a tool brings, as it needs them:

- **its panel**: the page column it fills in `WorldsmithView.vue` (the chat and the games dock stay the same for every tool);
- **its session words**: a paragraph in `src-tauri/resources/worldsmith-rules.md`, so the room's companion knows the tool and its limits;
- **its tables**: a new migration in `src-tauri/resources/schema/` (released migrations are never edited);
- **its commands**: Rust under `src-tauri/src/worldsmith/`, every one behind `rooms::need("worldsmith")`.

## Capabilities

The helper (`worldsmith-helper/`, released on its own `worldsmith-helper-v*` tag, its SHA-256 pinned by each app) says what it can do:

```
worldsmith-helper caps
{"ok":true,"version":"1.0.0","verbs":["games","attach",...],"features":["rect","thumb","unknown-scan","stamp"]}
```

The page reads `caps` after the helper starts and lights up only the tools whose `needs` are all in `features`. An older helper that does not know `caps` counts as having no features. New helper abilities are added as new verbs or new reply fields only; nothing a released app calls is ever renamed or removed.

## Worked example: Translate (the first Lens)

- **Manifest**: `{ id: 'translate', name: 'Translate', kind: 'lens', line: 'Read the game in your language', needs: ['rect', 'thumb'] }`.
- **Reading**: the helper's `shot` gives the picture and the game window's `rect`; the member's own Claude reads it in a one-shot (only allowed to read that one picture) and answers `<lines>[{x, y, w, h, text, translated}]</lines>` in picture pixels. The words in the picture are data, never instructions. The target language is the member's browser language.
- **Drawing**: a see-through, click-through, always-on-top window (`worldsmith-overlay`) is laid exactly on `rect`, and each translation floats beside its text as a glass speech bubble.
- **Keys and Auto**: F10 (held only while a game is attached) translates, and hides again; Auto compares a tiny `thumb` every 2 seconds, hides while the screen moves and reads again once it is still.
- **Memory**: `worldsmith_lines` keeps what was translated per game, and the newest 40 go back into the next read so names stay the same.
- **Limit**: an exclusive full-screen game cannot be drawn over; the page says to use borderless or windowed.

## Adding a tool, step by step

1. Choose its kind. If it needs to change memory, it is a Hammer; if it only reads the screen, a Lens; if it writes files, a Chisel.
2. Add its manifest to `TOOLS` with the helper features it `needs`.
3. If the helper must learn something new, add a verb or a reply field (additive only), list it in `caps`, and bump the helper version.
4. Build its panel, its commands and (if it remembers anything) its migration.
5. Add its paragraph to the room's rules and its section to the user guide (chapter 22).
6. Test the pure parts; check it live against a game in borderless mode.
