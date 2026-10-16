# MapleStory Classic World EXP Tracker — Design

Date: 2026-10-16
Status: Approved in brainstorming, pending spec review

## Goal

A Windows desktop app that measures EXP/hr for MapleStory Classic World by reading the screen, and records it per map without the player having to think about it. Primary output is a continuously logged rolling EXP/hr; history ties training time and rates to map names.

## Hard constraints (Nexon ToS)

Nexon prohibits third-party software that interacts with the game. The app is a passive screen reader, equivalent to OBS "Display Capture". These rules are non-negotiable and apply to every component:

1. Capture the **monitor** via Windows Graphics Capture (WGC). Never target the game window/HWND, never use injection-based capture.
2. Never open a handle to the game process, read its memory, load into it, hook it, or read its files (including WZ/data files).
3. Never send input (keys, clicks, focus changes).
4. No in-game overlay. The app is its own ordinary window.
5. If the game ever actively blocks capture, stop — do not circumvent.
6. Written confirmation from Nexon support is required before public release.

Verified 2026-10-15: WGC monitor capture of the primary monitor returned a clean 1920×1080 frame of the running Classic World client, no yellow border (`draw_border=false`, Windows 11).

## Stack

- **Tauri 2**, Windows only.
- **Rust backend:** capture (`windows-capture` crate, WGC), reading, tracker state machine, SQLite (`rusqlite`), error types with `thiserror`.
- **Frontend:** React + TypeScript (strict) + Vite, chart library (Recharts, version verified at implementation), Zod at the IPC boundary.

## Screen regions

Two regions are read from each frame. Observed layout at 1920×1080 (reference only — real coordinates come from calibration):

| Region | Content | Example | Reader |
|---|---|---|---|
| EXP | `326 [0.10%]` in the status bar, right of the `EXP.` label | x≈1048–1140, y≈1036–1052 | Glyph templates |
| Map name | Street + map name under the minimap title bar, minimap pinned top-left | `Victoria Road` / `Perion`, x≈62–300, y≈36–80 | Pixel signature (identity) + Windows OCR (label) |

- **Calibration** (one-time, saved per monitor resolution): the user drags a box over the EXP numbers (excluding the `EXP.` label, extending to the end of the bar so longer numbers fit). The map-name strip is a fixed offset from the monitor's top-left corner, so it gets a preset box (x 62, y 36, w 240, h 44) the user can adjust.
- **Map-name width:** the minimap's width changes per map. The name strip has a flat background (RGB 153,187,204 measured 2026-10-15). The reader scans a text-free row of the strip from the left until the colour departs from that background. That's the minimap frame's right edge, and the reader only looks up to it. No per-map size data is used: none is published, and extracting it from game files is prohibited.
- **Level is not read.** Level-ups are detected from the EXP readout itself (see Tracker). This removes a reader whose digit templates would need characters at many different levels to collect.

## Readers

All readers are pure functions: `crop -> Result<Reading, ReadError>`. A failed read is a normal outcome (loading screen, alt-tab, a window covering the region), not an error to surface.

- **EXP glyphs:** mark pixels with luminance ≥ 150 as text (measured 2026-10-15), trim the line vertically, split it into glyphs at empty columns, and match each glyph exactly against templates of the game's font. Template set: `0–9 . , % [ ]`, plus any multi-piece parts the splitter produces (e.g. `%`). A glyph with no match fails the read. Templates are stored as ASCII-art text in Rust source (`#`/`.` rows), not images. A dev-only binary (behind the `dev-tools` cargo feature, never in release builds) prints unseen glyphs as ASCII art from live capture or a PNG fixture, for labelling.
- **EXP output:** `exp: u64`, `pct_bp: u32` (percent in basis points, `0.10%` → `10`). Parsed from `<digits/commas> [ <d>.<dd> % ]`.
- **Map identity — pixel signature, not OCR.** Verified 2026-10-15: Windows OCR on the same strip returns `Victoria Road Perion`, `Victoria Road`, `Victoria Roa(l perion` or empty depending only on the upscale factor, so it can't be what identifies a map. The game draws a map's name identically on every frame, so the signature is the per-column count of text pixels (min channel ≥ 215) across the strip, up to the right edge. Two signatures match when they're the same length and the summed absolute difference is ≤ 4. A blank strip (no text pixels) is unreadable.
- **Map label — Windows OCR, voted.** When a new signature is first confirmed, OCR runs on the 3 confirming strips at several upscales (2× nearest, 2× cubic, 3× cubic). The normalized (trimmed, whitespace-collapsed) result that comes up most often becomes the map's name. The user can rename a map in History. The label is display only; identity is the signature.

## Tracker

Capture runs every **1 s**. Each frame produces a `Sample { t, exp: Option<ExpReading>, map: Option<MapId> }`, where `map` is the currently confirmed map.

- **Map confirmation:** a signature counts once it matches on 3 consecutive readable frames. Unreadable frames (loading screens, channel changes) are skipped and neither confirm nor break a streak.
- **Level total, as an interval:** each reading with `pct_bp ≥ 2` bounds the level's total EXP to `[exp × 10000 / (pct_bp + 1), exp × 10000 / (pct_bp − 1)]`, which allows for the game either rounding or truncating the %. The tracker intersects these bounds across the level, and the estimate `total_est` is the midpoint. They reset on level-up.
- **Consistency check:** a reading whose bounds don't overlap the current interval is a misread and is rejected, keeping the baseline.
- **EXP gain between the baseline and a new accepted reading:**
  - EXP up → gain = difference.
  - Level-up: baseline `pct_bp ≥ 8000`, new `pct_bp < 2000` and EXP down → gain = `(total_est − last_exp) + new_exp`. Just before a level-up the % is high, so `total_est` is tight there.
  - Any other EXP drop → death penalty. Gain = 0, and the baseline resets to the new value.
- **Training seconds:** a second counts as training when the player is on the session's confirmed map and EXP rose within the last 120 s. The gap between samples is capped at 3 s, so a stall in capture doesn't count as training. Repot trips, town, transit, AFK and unreadable time count as zero training seconds.
- **Sessions:** a session starts on the first EXP gain on a confirmed map. It continues while the player stays on that map, and continues if they leave and come back to the **same map within 10 minutes** (time away is excluded because it isn't training time). It ends when EXP is gained on a different map, or after 10 minutes away. Visits that never gain EXP never create a session. EXP gained before any map is confirmed can't be attributed and is not recorded.
- **Minute rollup:** at each wall-clock minute boundary, the tracker emits one row per session active in that minute: `exp_gained`, `training_secs`.
- The tracker is pure: it takes samples and returns effects (`SessionStarted`, `SessionEnded`, `Minute`). The pipeline applies effects to storage.

## Storage

- **No images are ever written to disk by the shipped app.** Frames and crops exist only in memory for the duration of a read, and are dropped once parsed. Only parsed numbers, names and map signatures (column counts) are stored. (The calibration screen's frozen frame is held in memory and sent to the UI as an in-memory PNG, never saved.)
- **One file, nothing to install.** Data lives in a single SQLite file, `%APPDATA%\<app>\tracker.db`. SQLite is compiled into the `.exe` (`rusqlite` with the `bundled` feature), so there is no database server, service, or runtime for the user to install. The installer ships one exe plus the WebView2 runtime Windows 11 already has. Deleting the file resets all history.
- SQLite over flat files (JSON/CSV): the per-map averages, session lookups and windowed chart queries are indexed queries on SQLite. Flat files would need reading and re-aggregating the full history on every query, and rewriting files to close a session.

Schema (versioned with `PRAGMA user_version`):

- `maps(id, name, signature BLOB, first_seen)`
- `sessions(id, map_id, started_at, ended_at NULL)`. Sessions still open at startup are closed at their last recorded minute.
- `minutes(minute_start, session_id, exp_gained, training_secs, PRIMARY KEY(minute_start, session_id))`
- `calibration(resolution TEXT PRIMARY KEY, exp_x, exp_y, exp_w, exp_h, map_x, map_y, map_w, map_h)`

12 hours of training is ~720 minute rows. All timestamps are UTC unix seconds.

## Rates and charts

Computed in Rust from minute rows and sent to the UI (one implementation, unit-tested):

- **Bars:** EXP gained per 10-minute wall-clock bucket, for a window of 120 or 180 minutes (12 or 18 bars).
- **Rate line:** 60-minute trailing SMA of EXP/hr = `Σ exp_gained / Σ training_secs × 3600` over the 6 buckets ending at each bar. It's empty when there's no training time. Buckets with zero training seconds contribute nothing, so repot trips don't drag the line down.
- **Live numbers:** current rate line value (EXP/hr), %/hr using `total_est`, time to level = `(total_est − exp) / rate`.

## UI

- **Live view:** current map, EXP/hr, %/hr, time to level, bar chart + rate line (120/180-minute toggle), reader status (OK / EXP unreadable / map unreadable / not calibrated / capture failed).
- **History:** per-map table (sessions, training hours, average EXP/hr, best 60-minute rate) with rename, and a session list. Selecting a session shows its bar chart.
- **Calibrate:** a frozen frame from the monitor, with drag-to-draw boxes for the EXP numbers and the map strip, and a preview of what each reader returns.

## IPC

- Event `tracker-tick` (each frame): reader status, current map name, latest EXP reading, `total_est`.
- Event `tracker-minute` (each minute): a signal to refetch the live chart.
- Commands: `get_live_chart(window_minutes)`, `get_map_summaries()`, `rename_map(map_id, name)`, `get_sessions(map_id?)`, `get_session_chart(session_id)`, `get_calibration_frame()`, `save_calibration(...)`, `preview_regions(...)`.
- Every payload is validated with Zod in the frontend. Commands return `Result<T, AppError>` (serialized as a tagged enum), and the TS wrapper converts the result to `Result<T, E>`.

## Error handling

- Rust: `thiserror` enums per module (`CaptureError`, `ReadError`, `StoreError`). Readers return `Result`, and the tracker treats `ReadError` as "unreadable sample". No `unwrap()` outside tests and invariants.
- Capture loss (monitor change, GPU device lost): restart the WGC session with backoff (1 s doubling to 30 s), and report `capture failed` status until it's back.
- Storage errors are reported to the UI status, never swallowed. Minute rows that failed to write stay queued in memory and are retried on the next minute.
- TS: `Result<T, E>` with `ok()`/`err()`, and no throwing for IPC failures.

## Testing

- **Readers:** fixture crops (EXP region and map strip only — no chat or other players) in `src-tauri/tests/fixtures/`. These are dev-only test assets in the repo, not shipped and not written by the app, starting with the 2026-10-15 capture (`326`, `0.10%`, `Victoria Road / Perion`). Add more as glyphs are collected.
- **Tracker:** pure state machine fed synthetic `Sample` sequences: normal gain, level-up, death, map debounce through unreadable frames, repot return within and after 10 minutes, AFK, a misread rejected by the consistency check.
- **Rates:** bucket and SMA functions against hand-computed minute rows.
- **Store:** in-memory SQLite round-trips.
- **Frontend:** Zod schemas, and the Result helpers.
- **Manual end-to-end:** an hour of real play, comparing the app's EXP gain against the start/end EXP shown in game.

## Out of scope (v1)

Reading the character level, multiple monitors (primary only), auto-locating regions without calibration, start with Windows / tray mode, always-on-top mini overlay, map-name dataset from third parties (MeowDB — would need permission), per-mob/kill analytics, exporting data, macOS.
