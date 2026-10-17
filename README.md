# Maple EXP Tracker

A Windows desktop app that measures EXP/hr in MapleStory Classic World by reading the screen. Once a second, it captures the primary monitor (the same way OBS "Display Capture" does). It reads the EXP readout, the level and the minimap's map name from that image in memory, and records EXP gained per map in a local SQLite file.

**The app never touches the game.** It does not open, read, inject into or hook the MapleStory process, read game files, target the game window, send keyboard or mouse input, or draw an overlay. It never writes screenshots to disk.

## How it works

```mermaid
%%{init: {"flowchart": {"wrappingWidth": 320}}}%%
flowchart TB
    subgraph outside["Not part of the app"]
        game["MapleStory.exe<br/>never opened, read, hooked or sent input"]
        monitor["Primary monitor<br/>(the picture on screen)"]
        game -. "draws to" .-> monitor
    end

    subgraph backend["src-tauri/src (Rust)"]
        capture["capture.rs<br/>Windows Graphics Capture of the monitor, 1 frame/s<br/>copies 3 small boxes into memory, drops the rest"]

        subgraph pipeline["pipeline.rs"]
            exp["read/exp.rs + read/glyphs.rs<br/>EXP box to 326 and 0.10%<br/>(greyscale glyph matching, no OCR)"]
            strip["read/map_strip.rs<br/>map box to pixel signature"]
            lvl["read/level.rs<br/>level box to 41, using digits<br/>learned while playing"]
            confirm["tracker/confirm.rs<br/>map confirmed after<br/>3 matching frames"]
            ocr["read/ocr.rs<br/>Windows OCR: display name<br/>for a newly seen map"]
            track["tracker/mod.rs + tracker/level.rs<br/>EXP gained, level-up, death,<br/>sessions, training time per minute"]
        end

        store[("store.rs: SQLite, one file<br/>%APPDATA%/com.mapleexp.tracker/tracker.db")]
        commands["commands.rs: Tauri commands<br/>+ rates.rs: 10-min bars, 60-min EXP/hr"]
    end

    ui["React UI: src/<br/>lib/ipc.ts validates every message (Zod)<br/>views/: Live, History, Calibrate"]

    monitor --> capture
    capture --> exp
    capture --> strip
    capture --> lvl
    lvl -- "screen level" --> track
    track -. "known level labels<br/>new digits" .-> lvl
    strip --> confirm
    confirm -- "new map" --> ocr
    exp --> track
    confirm --> track
    track -- "sessions and minute rows" --> store
    ocr -- "map name" --> store
    store --> commands
    commands <-- "invoke" --> ui
    pipeline -- "event: tracker-tick (1/s)" --> ui
    ui -. "Calibrate: request one frame,<br/>save box positions" .-> capture

    classDef offlimits stroke:#c62828,stroke-width:2px,stroke-dasharray:5 3
    class game offlimits
```

### Reading the diagram

- **Input.** The only input is the monitor image. `capture.rs` asks Windows for frames of the primary monitor (`Monitor::primary()`), never a window. It copies three small rectangles (EXP, map name, level) out of each frame and drops the rest.
- **Reading.** Readers in `read/` are pure functions (no I/O): pixels in, values out. Each EXP glyph is centred on its centre of brightness and compared with one greyscale reference per character, stored as ASCII art in `glyphs.rs`; a glyph that isn't clearly one character fails the read rather than guessing. The map is identified by its pixel signature, and OCR only produces a display name the user can rename. The level digits (`read/level.rs`) have no built-in references, because only the current level's digits are ever on screen: when the level box shows a digit the app hasn't seen while the level is already known from the EXP readings, the app labels it from that level and stores it in the database as the same kind of ASCII art.
- **Pipeline.** `pipeline.rs` is one background thread that runs the readers and tracker on each frame. It is the only place that writes to the database.
- **Tracking.** `tracker/` has no I/O. It turns each second's reading into effects (`SessionStarted`, `SessionEnded`, `Minute`), which `pipeline.rs` writes to SQLite. Repot trips and AFK time count as zero training seconds, so they don't lower the rate. `tracker/level.rs` holds the EXP-per-level table (a community table, **not confirmed by Nexon**; corrections welcome). The level is the one whose table total fits the EXP and % readings; a level read from the screen is used only when its table total fits them too.
- **UI.** The UI only talks to the backend through Tauri commands and one event (`tracker-tick`, once a second). Every payload is validated with Zod in `lib/ipc.ts`. Its look follows [`docs/STYLE.md`](docs/STYLE.md); the backdrops in `public/` are MapleStory wallpapers, chosen in the side nav (or none).
- **Logs.** A text log in `%LOCALAPPDATA%\com.mapleexp.tracker\logs\` (5 files of up to 1 MB) records reader failures with the partially read text, map and session changes, tracker decisions (level-up, death, rejected misreads), capture restarts and database errors, plus one summary line per minute. It contains text only, never images.
- **Dev tool.** `src/bin/glyph_collect.rs` is a developer-only tool that records the EXP box as text, replays recordings through the reader, and prints glyphs the reader rejects. It is built only with the `dev-tools` feature and is not part of the release build.

## Development

```bash
pnpm install
pnpm tauri dev                                  # run the app
cargo test --manifest-path src-tauri/Cargo.toml # reader + tracker tests
pnpm tauri build                                # NSIS installer
```
