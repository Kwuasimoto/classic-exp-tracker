# Classic EXP Tracker

A Windows desktop app that measures EXP/hr in MapleStory Classic World by reading the screen. Once a second, it captures the primary monitor (the same way OBS "Display Capture" does). It reads the EXP readout, the level and the minimap's map name from that image in memory, and records EXP gained per map in a local SQLite file.

**The app never touches the game.** It does not open, read, inject into or hook the MapleStory process, read game files, target the game window, send keyboard or mouse input, or draw an overlay. It never writes screenshots to disk.

```mermaid
flowchart LR
    game["MapleStory<br/>runs as normal"] -- "shows the game on" --> monitor["Your monitor"]
    monitor -- "a picture once a second<br/>(like OBS Display Capture)" --> app["Classic EXP Tracker<br/>reads EXP, level and map name<br/>off the picture, then discards it"]
    app -- "numbers only" --> file["A file on your PC<br/>(the last 3 hours, plus what you save)"]
    never["Never: reads game memory or files,<br/>sends keys or clicks, draws over the game"]
    game ~~~ never

    classDef no stroke:#c62828,stroke-width:2px,stroke-dasharray:5 3
    class never no
```

> **Status:** submitted to Nexon for review against the MapleStory Terms of Service. It won't be released publicly until Nexon confirms in writing that it's allowed.

## Install

1. Download `Classic EXP Tracker_<version>_x64-setup.exe` from the [Releases page](https://github.com/Kwuasimoto/classic-exp-tracker/releases).
2. Run it. The installer isn't code-signed, so Windows SmartScreen may say "Windows protected your PC": choose **More info**, then **Run anyway**.
3. It installs for your user account only (no administrator rights) and adds a Start menu entry.

**Requirements:** Windows 10 version 2004 or later, or Windows 11, 64-bit. The installer downloads Microsoft Edge WebView2 if it's missing (Windows 11 already has it). On Windows 10, Windows draws a yellow border around the screen while the app captures it; Windows 11 lets the app hide it. Map names are read with Windows' built-in text recognition, which needs a Windows display language that supports it, such as English.

## Set up

1. **Set Windows to a 16:9 resolution and play MapleStory full screen** on your primary monitor. **1366×768** is the game's own resolution: its text is drawn pixel for pixel there and reads best. Larger 16:9 sizes such as 1920×1080 work too. Other shapes, such as 1280×960 or ultrawide, aren't supported yet: the game stretches its text unevenly there.
2. Keep the status bar (EXP and level) and the minimap visible, with the minimap showing the map name. The app reads only the primary monitor. If you have a second monitor, the app opens there so it doesn't cover the game; with one monitor, keep the app's window away from the EXP bar and minimap.

There's nothing to calibrate: the app knows where the game draws EXP, the map name and the level, and scales those spots to your screen. EXP is read from the game's own pixel font, so there's nothing to teach. The level digits are learned by themselves while you play, per screen resolution.

## Use

- **Live** shows your EXP/hr over the last 60 minutes of training, %/hr, time to level and EXP to level, with a chart of EXP per 10 minutes. Time in town or AFK doesn't lower the rate.
- The **save** button left of 2H/3H saves what the chart shows to History as a snapshot. The **reset** button next to it starts a new session and saves nothing.
- Only one copy of the app runs at a time; opening it again brings the open window forward.
- The map's name comes from a list of the game's map names. A map not on that list shows as **Unrecognised map** and is still tracked; the pencil next to the name lets you name it (or rename any map), and the app never asks you to.
- In shops the game collapses the minimap to a grey title bar; the app shows **Shopping** then, and the time doesn't count as training.
- **History** lists your snapshots, which can be renamed and deleted. Snapshots are the only history kept: live data is deleted once the 3H chart no longer shows it, so save anything you want to keep.
- The status line at the bottom of the side nav says whether reading works. If it doesn't:
  - **EXP not readable:** the game isn't full screen, or the status bar is hidden or covered.
  - **Map name not readable:** open the minimap.
  - **… isn't 16:9:** set Windows to a 16:9 resolution, such as 1366×768 or 1920×1080.
  - **Screen capture failed:** another app or the game is blocking capture. The app stops rather than working around it.

## Your data

Everything stays on your PC; the app makes no network requests.

- History: `%APPDATA%\com.classicexp.tracker\tracker.db`
- Logs (text only): `%LOCALAPPDATA%\com.classicexp.tracker\logs\`

To uninstall, use **Settings > Apps > Installed apps > Classic EXP Tracker**. Your history is kept unless you tick **Delete the application data** in the uninstaller.

## How it works (for reviewers and developers)

- **Input.** The only input is the monitor image. `capture.rs` asks Windows for frames of the primary monitor (`Monitor::primary()`), never a window. It copies three small rectangles (EXP, map name, level) out of each frame and drops the rest. The rectangles are fixed spots measured at the game's own resolution, 1366×768, and scaled to the monitor (`geom.rs`); that works because the game stretches its whole screen to the monitor, which keeps the spots in proportion on any 16:9 screen.
- **Reading.** Readers in `read/` are pure functions (no I/O): pixels in, values out. The EXP readout is drawn in a 5×7 pixel font scaled to the screen. `read/font.rs` holds that font and recognises each glyph by drawing every character at the line's measured scale, at sub-pixel offsets, and comparing the drawing with the screen by normalised correlation. A glyph that isn't clearly one character fails the read rather than guessing. Glyphs that touch are cut apart by the font's pitch (`read/exp.rs`). The map is identified by the pixel signature of its name, and a map can have several (one per resolution and minimap width). Its name is read with Windows OCR and accepted only if it matches a name in `data/maps.txt`, the game's map names from the MapleStory Classic Wiki: exactly, or within a misread letter when one name is clearly closest (`read/map_names.rs`). Anything else stays unrecognised rather than shown misread, and is read again a few times while you stay on the map. The level digits (`read/level.rs`) don't start with any glyphs, because only the current level's digits are ever on screen: when the level box shows a digit the app hasn't seen while the level is already known from the EXP readings, the app labels it from that level and stores it in the database (table `glyphs`) as ASCII art. Until then the level comes from the EXP readings alone.
- **Pipeline.** `pipeline.rs` is one background thread. For each frame, `screen/` runs the readers, confirms map changes, follows the level box and logs reader failures, without any I/O; `pipeline.rs` then feeds the tracker, names new maps with OCR and saves the results. It is the only place that writes to the database.
- **Tracking.** `tracker/` does no I/O apart from log lines explaining its decisions. It turns each second's reading into effects (`SessionStarted`, `SessionEnded`, `Minute`), which `pipeline.rs` writes to SQLite. Repot trips and AFK time count as zero training seconds, so they don't lower the rate. `tracker/level.rs` holds the EXP-per-level table (a community table, **not confirmed by Nexon**; corrections welcome). The level is the one whose table total fits the EXP and % readings; a level read from the screen is used only when its table total fits them too.
- **UI.** The UI only talks to the backend through Tauri commands and one event (`tracker-tick`, once a second). Every payload is validated with Zod in `lib/ipc.ts`. The save button on the Live chart copies what the chart shows into a snapshot (listed in History, renamable and deletable); the reset button starts a new session. Stored minutes older than the 3H chart needs are deleted as new ones are saved, so snapshots are the only lasting history. Its look follows [`docs/STYLE.md`](docs/STYLE.md); the backdrops in `public/` are MapleStory wallpapers, chosen in the side nav (or none).
- **Logs.** A text log in `%LOCALAPPDATA%\com.classicexp.tracker\logs\` (5 files of up to 1 MB) records reader failures with the partially read text, map and session changes, tracker decisions (level-up, death, rejected misreads), capture restarts and database errors, plus one summary line per minute. It contains text only, never images.
- **Dev tool.** `src/bin/glyph_collect.rs` is a developer-only tool that records the EXP box as text, replays recordings through the reader, prints each glyph's match scores, and prints the glyphs it can't read with their closest characters. It is built only with the `dev-tools` feature and is not part of the release build.

## Development

```bash
pnpm install
pnpm tauri dev                                  # run the app
cargo test --manifest-path src-tauri/Cargo.toml # reader + tracker tests
pnpm tauri build                                # NSIS installer
```

Needs Node.js 20.19+ or 22.12+, pnpm, the Rust toolchain (stable, MSVC) and the [Tauri prerequisites for Windows](https://tauri.app/start/prerequisites/). The installer is written to `src-tauri/target/release/bundle/nsis/`.

To release, set the same version in `package.json`, `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json`, run `pnpm tauri build`, and attach the installer to a GitHub release.

## Credits and license

Two other open trackers shaped the EXP reader and the map naming. No code was copied from either:

- The idea that the EXP text is a 5×7 pixel font the game scales up, read as that font at any scale, comes from xEmPIr3's MapleStory Classic EXP tracker.
- Matching OCR reads of map names against a list of the game's map names, allowing a misread letter only when one name is clearly closest and the numbers agree, also follows beanguri's tracker.
- Comparing glyphs by normalised correlation with a little shift tolerance (and its score limits), and cutting touching digits by the font's pitch, come from [beanguri/Maple-Classic-Exp-Overlay](https://github.com/beanguri/Maple-Classic-Exp-Overlay) (MIT).

The map names in `src-tauri/data/maps.txt` come from the [MapleStory Classic Wiki](https://mapleclassic.wiki/wiki/Category:Maps) and are licensed [CC BY-NC-SA 4.0](https://creativecommons.org/licenses/by-nc-sa/4.0/) by its contributors (see `src-tauri/data/LICENSE.md`).

The backgrounds in `public/` are official MapleStory wallpapers; MapleStory and its artwork are © Nexon. This project isn't affiliated with or endorsed by Nexon.

Copyright © 2026 Kwuasimoto. All rights reserved. The source is published for review and transparency; no license is granted to reuse it.
