# worldsmith-helper

The small memory helper behind **WORLDSMITH**, the AI cheat engine in MemoryCore Castle and the
Alice Keep. It finds and changes values (gold, HP, ammo, timers) in **your own offline
single-player games** on Windows. MIT licence, like the rest of this repo.

## Safety limits

- User mode only: `OpenProcess`, `VirtualQueryEx`, `ReadProcessMemory`, `WriteProcessMemory`,
  with your own rights. No kernel driver, no code injection, no remote threads, no DLL loading,
  no debugger, no anti-cheat bypass of any kind.
- `attach` refuses a game that has an anti-cheat module loaded (EasyAntiCheat, BattlEye, Riot
  Vanguard, nProtect GameGuard, XIGNCODE3, EOS Anti-Cheat or any module naming itself anti-cheat),
  a game whose modules can't be listed, and a game with a live ESTABLISHED TCP link to a
  non-loopback address. `games` shows the same verdict per game. (Valve's VAC can't be seen from
  user mode.)
- When the parent app exits, the game exits or you `detach`, every freeze stops and the handle is
  dropped.

## How it runs

`worldsmith-helper serve --parent <pid>` starts the one server for this user, on the named pipe
`\\.\pipe\worldsmith-<USERNAME>` (`WORLDSMITH_PIPE` overrides the name, for tests). It exits when
the parent process dies; a second `serve` prints `{"ok":true,"already":true}` and exits.

Every other call is a client: `worldsmith-helper <verb> <args>` sends one JSON line to the server
and prints one JSON line. Every reply has `"ok"`; failures carry one plain `"error"` line.
Addresses are `0x` + uppercase hex. With no server running, the reply is
`{"ok":false,"error":"Worldsmith isn't running - open the WORLDSMITH room first."}`.

| Verb | Reply |
| --- | --- |
| `games` | `games: [{pid, exe, title, stamp, refused, reason}]` (windowed processes; `stamp` = exe `<size>-<mtime>`) |
| `attach <pid>` | `{pid, exe, title}` or the refusal |
| `detach` | stops freezes, drops the handle and every label |
| `shot` | `{path}` - a PNG of the game's window in `%TEMP%\worldsmith-shots\shot-<unix ms>.png` |
| `scan --label L --type i32\|i64\|f32\|f64\|auto --value V [--transform x10\|x100\|neg]` | `{label, matches, steps}` (+ `top` when 20 or fewer) |
| `scan --label L --type i32\|i64\|f32\|f64 --unknown` | the same; every aligned value of that type is a candidate |
| `next --label L --value V \| --changed \| --unchanged \| --increased \| --decreased` | `{label, matches, steps, top?}` |
| `list --label L --max 20` | `{label, items: [{address, type, value}]}` |
| `write <addr> <type> <value>` | `{}` |
| `freeze <addr> <type> <value>` / `unfreeze <addr>` | `{}` (frozen values are rewritten every 50 ms) |
| `state` | `{attached, labels: [{label, steps, top}], freezes: [{address, type, value}]}` |

## Decisions

- Scans read committed, writable (`PAGE_READWRITE`, `PAGE_WRITECOPY`, `PAGE_EXECUTE_READWRITE`,
  `PAGE_EXECUTE_WRITECOPY`; not guard / no-access) private and image memory, in 1 MB chunks, at
  4-byte aligned addresses for every type.
- `auto` tries i32, f32 and f64 and records each candidate's type.
- The transform turns the shown value into the stored one before comparing (`x10`: stored = shown
  x 10, `neg`: stored = -shown). Integers compare exactly.
- Floats match when `|stored - target| <= max(0.5 x |factor|, |target| x 0.001)` - the 0.5 covers a
  value the game shows rounded, the factor is the transform's (1, 10, 100).
- A label keeps at most 5,000,000 candidates; a first scan stops there and adds `"capped":true`.
- `--unknown` keeps a copy of the scanned bytes plus one bit per aligned slot (about 1.03 bytes per
  byte of game memory, capped at 1 GiB copied; `"capped":true` when regions were left out). Each
  `next` compares against the copy from the previous step. Once a list is smaller than the copy,
  the label becomes an ordinary candidate list.
- `shot` uses Windows.Graphics.Capture, and falls back to GDI (`PrintWindow`, then a screen copy of
  the window's monitor) when that fails. The reply also says which one was used (`"via"`).

## Build and test

```
cargo test --manifest-path worldsmith-helper/Cargo.toml
cargo build --release --manifest-path worldsmith-helper/Cargo.toml
```

The end-to-end test runs a real server against `examples/target_game.rs` on its own pipe name.
A `worldsmith-helper-v*` tag publishes `worldsmith-helper.exe` and its `.sha256` as a GitHub release.
