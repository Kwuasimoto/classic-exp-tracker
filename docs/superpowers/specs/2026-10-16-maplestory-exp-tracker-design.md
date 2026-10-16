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

Three regions are read from each frame. Observed layout at 1920×1080 (reference only — real coordinates come from calibration):

| Region | Content | Example | Reader |
|---|---|---|---|
| EXP | `EXP. 326 [0.10%]` in the status bar | x≈1010–1190, y≈1030–1060 | Digit templates |
| Level | `LV. 41`, orange digit tiles | x≈405–500, y≈1030–1080 | Digit templates |
| Map name | Street + map name under the minimap title bar, minimap pinned top-left | `Victoria Road` / `Perion` | Windows OCR |

- **Calibration** (one-time, saved per monitor resolution): the user drags a box over the EXP readout and the level. The map-name strip is a fixed offset from the monitor's top-left corner, so it gets a preset box the user can adjust.
- **Map-name width:** the minimap's width changes per map. The reader scans along the name strip for the minimap frame's border colour to find the right edge, and reads only up to it. No per-map size data is used: none is published, and extracting it from game files is prohibited.

## Readers

All readers are pure functions: `crop -> Result<Reading, ReadError>`. A failed read is a normal outcome (loading screen, alt-tab, a window covering the region), not an error to surface.

- **Digits (EXP, %, level):** binarize the crop, split it into glyphs by column, and match each glyph against stored template images of the game's bitmap font. Glyphs: `0–9 . , % [ ]` for EXP; `0–9` for level. Matching must exceed a similarity threshold, or the read fails. Templates are cut from real captures during development and compiled into the binary (`include_bytes!`). A dev-only binary (excluded from release builds) saves region crops to collect the full glyph set.
- **EXP output:** `exp: u64`, `pct_bp: u32` (percent in basis points, `0.10%` → `10`). Consistency check: once the level's total EXP is estimated, `exp / total` must agree with `pct_bp` within rounding, or the read is rejected.
- **Map name:** Windows built-in OCR (`Windows.Media.Ocr` via the `windows` crate) on the upscaled crop. Result is normalized (trim, collapse whitespace) and fuzzy-matched (edit distance ≤ 2) against map names seen before. A close match snaps to the known name. An unmatched name becomes a candidate.

## Tracker

Capture runs every **1 s**. Each frame produces a `Sample { t, exp: Option<(u64, u32)>, level: Option<u32>, map: Option<String> }`.

- **Map confirmation:** a map name counts once it's read identically on 3 consecutive readable frames. Unreadable frames (loading screens, channel changes) are skipped and neither confirm nor break a streak.
- **EXP gain between consecutive valid readings:**
  - Same level, EXP up → gain = difference.
  - Level up by 1 (level confirmed on 2 frames) → gain = `(total_est − last_exp) + new_exp`, where `total_est = last_exp × 10000 / last_pct_bp`. This is accurate because the last reading before a level-up has a high %.
  - Same level, EXP down → death penalty. Gain = 0, and the baseline resets to the new value.
  - Any other jump (level up by more than 1, implausible gain) → reading rejected, baseline kept.
- **Level total estimate** (`total_est`) is refreshed from every valid reading with `pct_bp ≥ 5000`. It powers time-to-level and the consistency check.
- **Training seconds:** a second counts as training when the player is on a confirmed map and EXP rose within the last 120 s. Repot trips, town, transit, AFK and unreadable time count as zero training seconds.
- **Sessions:** a session starts on the first EXP gain on a confirmed map. It continues while the player stays on that map, and continues if they leave and come back to the **same map within 10 minutes** (time away is excluded because it isn't training time). It ends when EXP is gained on a different map, or after 10 minutes away. Visits that never gain EXP never create a session.
- **Minute rollup:** every wall-clock minute, the tracker writes one row per session active in that minute: `exp_gained`, `training_secs`, `level` at end of minute.

## Storage

- **No images are ever written to disk by the shipped app.** Frames and crops exist only in memory for the duration of a read, and are dropped once parsed. Only parsed numbers and names are stored. (The calibration screen's frozen frame is held in memory and sent to the UI, never saved.)
- **One file, nothing to install.** Data lives in a single SQLite file, `%APPDATA%\<app>\tracker.db`. SQLite is compiled into the `.exe` (`rusqlite` with the `bundled` feature), so there is no database server, service, or runtime for the user to install. The installer ships one exe plus the WebView2 runtime Windows 11 already has. Deleting the file resets all history.
- SQLite over flat files (JSON/CSV): the per-map averages, session lookups and windowed chart queries are indexed queries on SQLite. Flat files would need reading and re-aggregating the full history on every query, and rewriting files to close a session.

Schema:

- `maps(id, name UNIQUE, first_seen)`
- `sessions(id, map_id, started_at, ended_at NULL)`
- `minutes(minute_start, session_id, exp_gained, training_secs, level, PRIMARY KEY(minute_start, session_id))`
- `calibration(resolution TEXT PRIMARY KEY, exp_rect, level_rect, map_rect)`

12 hours of training is ~720 minute rows. All timestamps are UTC unix seconds.

## Rates and charts

Computed in Rust from minute rows and sent to the UI (one implementation, unit-tested):

- **Bars:** EXP gained per 10-minute wall-clock bucket, for a window of 120 or 180 minutes (12 or 18 bars).
- **Rate line:** 60-minute trailing SMA of EXP/hr = `Σ exp_gained / Σ training_secs × 3600` over the 6 buckets ending at each bar. Buckets with zero training seconds contribute nothing, so repot trips don't drag the line down.
- **Live numbers:** current rate line value (EXP/hr), the same as %/hr using `total_est`, time to level = `(total_est − exp) / rate`.

## UI

- **Live view:** current map, EXP/hr, %/hr, time to level, session duration, bar chart + rate line (120/180-minute toggle), reader status (OK / EXP not found / map unreadable / capture failed).
- **History:** per-map table (sessions, training hours, average EXP/hr, best 60-minute rate), and a session list. Selecting a session shows its bar chart.
- **Calibrate:** a frozen frame from the monitor, with drag-to-draw boxes for the EXP and level regions and an adjustable map strip, and a live preview of what each reader returns.

## IPC

- Event `tracker://tick` (each frame): reader status, current map, latest EXP/level.
- Event `tracker://minute` (each minute): updated live chart series and live numbers.
- Commands: `get_live_chart(window_minutes)`, `get_map_summaries()`, `get_sessions(map_id?)`, `get_session_chart(session_id)`, `get_calibration_frame()`, `save_calibration(...)`, `preview_regions(...)`.
- Every payload is validated with Zod in the frontend. Commands return `Result<T, AppError>` (serialized as a tagged enum) and are narrowed on the TS side.

## Error handling

- Rust: `thiserror` enums per module (`CaptureError`, `ReadError`, `StoreError`). Readers return `Result`, the tracker treats `ReadError` as "unreadable sample". No `unwrap()` outside tests and invariants.
- Capture loss (monitor change, GPU device lost): restart the WGC session with backoff, and report `capture failed` status until it's back.
- Storage errors are reported to the UI status, never swallowed. The tracker keeps an in-memory minute buffer and retries the write.
- TS: `Result<T, E>` with `ok()`/`err()`, and no throwing for IPC failures.

## Testing

- **Readers:** fixture crops (EXP, level and map name only — no chat or other players) in `src-tauri/tests/fixtures/`. These are dev-only test assets in the repo, not shipped and not written by the app, starting with the 2026-10-15 capture (`326`, `0.10%`, `41`, `Victoria Road / Perion`). Add more as glyphs are collected.
- **Tracker:** pure state machine fed synthetic `Sample` sequences: normal gain, level-up, death, map debounce through unreadable frames, repot return within and after 10 minutes, AFK, a misread rejected by the consistency check.
- **Rates:** bucket and SMA functions against hand-computed minute rows.
- **Store:** in-memory SQLite round-trips.
- **Frontend:** Zod schemas, and the Result helpers.
- **Manual end-to-end:** an hour of real play, comparing the app's EXP gain against the start/end EXP shown in game.

## Out of scope (v1)

Multiple monitors (primary only), auto-locating regions without calibration, start with Windows / tray mode, always-on-top mini overlay, map-name dataset from third parties (MeowDB — would need permission), per-mob/kill analytics, exporting data, macOS.
