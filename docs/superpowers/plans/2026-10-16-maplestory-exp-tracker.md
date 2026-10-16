# MapleStory Classic World EXP Tracker Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use subagent-driven-development (recommended) or executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Windows Tauri app that reads the MapleStory Classic World EXP readout and minimap name from the primary monitor every second, and records EXP/hr per map into a single SQLite file, with live and history charts.

**Architecture:** A Rust backend runs a capture thread (Windows Graphics Capture, primary monitor) that crops two calibrated regions in memory and hands them to a pipeline thread. The pipeline runs pure readers (EXP glyph templates, map-strip pixel signature, Windows OCR for map labels) and a pure tracker state machine, and applies the tracker's effects to SQLite. A React + TS frontend renders live and history views from Tauri commands and events.

**Tech Stack:** Tauri 2.12 (Rust 1.97), `windows-capture` 2.0.1, `windows` 0.62.2 (Media.Ocr), `rusqlite` 0.40.2 (`bundled`), `image` 0.25.10 (png only), `thiserror` 2.0.21, `base64` 0.23.1, React + TypeScript (strict) + Vite, Recharts 3.10.1, Zod 4.6.7, Vitest 5.0.5, pnpm 12.

**Spec:** `docs/superpowers/specs/2026-10-16-maplestory-exp-tracker-design.md`. Read it before starting any task.

## Global Constraints

- **Minimal tests.** The repo goes to Nexon for ToS review, so it must read straightforwardly. Tests cover only reading the committed fixtures and the tracker's EXP rules (level-up, death, repot trip) plus the rate line. No frontend tests.

- **Nexon ToS (non-negotiable):** capture the **monitor** via WGC only — never a window/HWND, never injection. Never open a handle to the game process, read its memory or files, hook it, or send input. No in-game overlay. If the game blocks capture, stop and report — do not circumvent.
- **No images are ever written to disk by the shipped app.** Frames and crops exist only in memory. Only the dev-only `glyph-collect` binary (cargo feature `dev-tools`) and committed test fixtures involve image files.
- **One data file:** `%APPDATA%\com.mapleexp.tracker\tracker.db`, SQLite bundled into the exe (`rusqlite` feature `bundled`). No WAL mode (it creates extra files).
- Capture interval: **1 s**. Map confirmation: **3** consecutive matching readable frames. Training grace: **120 s**. Sample gap cap: **3 s**. Return window: **600 s**. Bars: **10-minute** buckets; window **120 or 180** minutes; rate line: **60-minute** trailing SMA (6 buckets) = Σexp / Σtraining_secs × 3600.
- Level-up rule: baseline `pct_bp ≥ 8000`, new `pct_bp < 2000`, EXP decreased. Any other EXP drop is death (gain 0, baseline reset).
- Measured constants (2026-10-15 fixture): EXP text luminance ≥ **150**; map strip background RGB **(153,187,204)** ± **6**; map text min channel ≥ **215**; signature match tolerance **4**; default map strip rect **x 62, y 36, w 240, h 44**.
- Rust: `thiserror` error enums, `Result` everywhere; `unwrap`/`expect`/`panic` only in tests or for true invariants (with a message saying which invariant). Never log-and-ignore.
- TS: `strict` + `noUncheckedIndexedAccess`; `Result<T, E>` (`ok()`/`err()`) for every IPC call; Zod validates every payload from Rust.
- Windows only. Event names: `tracker-tick`, `tracker-minute`.
- Every commit message ends with:
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  ```

All `cargo` commands run from the repo root with `--manifest-path src-tauri/Cargo.toml`. All `pnpm` commands run from the repo root.

---

## File Structure

```
.gitattributes                         line endings (LF), PNG binary
src-tauri/
  Cargo.toml                           deps, dev-tools feature, glyph-collect bin
  tauri.conf.json                      product name, window, nsis bundle
  capabilities/default.json            core:default only
  src/main.rs                          (scaffold) calls lib::run
  src/lib.rs                           module tree, now_unix(), Tauri builder
  src/geom.rs                          Rect, Calibration, Crop (owned RGBA), resolution_key
  src/read/mod.rs                      ReadError, submodules
  src/read/glyphs.rs                   EXP font templates as ASCII art
  src/read/exp.rs                      segment → classify → parse → ExpReading
  src/read/map_strip.rs                right-edge scan + per-column text signature
  src/read/ocr.rs                      Windows OCR label vote (labels only)
  src/tracker/mod.rs                   Tracker: samples → effects (pure)
  src/tracker/level.rs                 TotalBounds interval math
  src/tracker/confirm.rs               MapConfirmer (3-frame debounce)
  src/rates.rs                         10-min bars, 60-min SMA, best-hour rate (pure)
  src/store.rs                         SQLite schema + queries
  src/capture.rs                       WGC handler, Shared, CaptureMsg, start()
  src/pipeline.rs                      AppState, Tick, pipeline thread
  src/error.rs                         AppError (IPC error enum)
  src/commands.rs                      Tauri commands
  src/bin/glyph_collect.rs             dev-only glyph harvesting tool
  tests/fixtures/exp_326_0.10.png      (committed) EXP crop, rect 1051,1035,140,19 of a 1920×1080 frame
  tests/fixtures/map_victoria_road_perion.png  (committed) map strip, rect 62,36,240,44
src/
  App.tsx, App.css                     shell + tabs + theme tokens
  lib/result.ts                        Result/ok/err
  lib/ipc.ts                           Zod schemas + typed invoke/listen wrappers
  lib/format.ts                        number/time/status formatting
  lib/live.ts                          liveRates() derivation
  lib/*.test.ts                        Vitest tests
  components/RateChart.tsx             bars + rate line (Recharts)
  views/LiveView.tsx, HistoryView.tsx, CalibrateView.tsx
```

---

### Task 1: Scaffold the Tauri app

**Files:**
- Create: everything from `create-tauri-app` (react-ts, pnpm), `.gitattributes`
- Modify: `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, `src-tauri/capabilities/default.json`, `package.json`, `tsconfig.json`, `src-tauri/src/lib.rs`
- Keep: `src-tauri/tests/fixtures/*.png` (already in the repo working tree)

**Interfaces:**
- Produces: library crate `maple_exp_tracker_lib` with `pub fn run()`; binary `maple-exp-tracker`.

- [ ] **Step 1: Scaffold into a temp dir and copy in**

The repo root already has `docs/` and `src-tauri/tests/fixtures/`, so scaffold elsewhere and merge.

```bash
set -euo pipefail
REPO="$(pwd)"
TMP="$(mktemp -d)"
cd "$TMP"
pnpm dlx create-tauri-app@4.7.5 maple-exp-tracker --template react-ts --manager pnpm --identifier com.mapleexp.tracker --yes
cp -r maple-exp-tracker/. "$REPO/"
cd "$REPO"
ls src-tauri/src src-tauri/tests/fixtures
```
Expected: `lib.rs main.rs` and both fixture PNGs listed.

- [ ] **Step 2: Repo hygiene**

Create `.gitattributes`:
```
* text=auto eol=lf
*.png binary
*.ico binary
*.icns binary
```

- [ ] **Step 3: Remove the template's opener plugin and greet demo**

In `src-tauri/Cargo.toml` delete the `tauri-plugin-opener` dependency line. In `package.json` delete the `@tauri-apps/plugin-opener` dependency. In `src-tauri/capabilities/default.json` set `"permissions": ["core:default"]`. Replace `src-tauri/src/lib.rs` with:

```rust
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

Replace `src/App.tsx` with a placeholder that compiles (it is rewritten in Task 12):
```tsx
export default function App() {
  return <main>Maple EXP Tracker</main>;
}
```
Delete `src/assets/` and any template CSS imports that no longer resolve (`src/App.css` is recreated in Task 12; delete it now).

- [ ] **Step 4: Add Rust dependencies and the dev-tools feature**

```bash
cargo add --manifest-path src-tauri/Cargo.toml serde@1 --features derive
cargo add --manifest-path src-tauri/Cargo.toml serde_json@1
cargo add --manifest-path src-tauri/Cargo.toml windows-capture@2.0.1
cargo add --manifest-path src-tauri/Cargo.toml windows@0.62.2 --features Media_Ocr,Graphics_Imaging,Storage_Streams,Foundation,Globalization
cargo add --manifest-path src-tauri/Cargo.toml rusqlite@0.40.2 --features bundled
cargo add --manifest-path src-tauri/Cargo.toml thiserror@2.0.21
cargo add --manifest-path src-tauri/Cargo.toml image@0.25.10 --no-default-features --features png
cargo add --manifest-path src-tauri/Cargo.toml base64@0.23.1
```

Append to `src-tauri/Cargo.toml`:
```toml
[features]
dev-tools = []

[[bin]]
name = "glyph-collect"
path = "src/bin/glyph_collect.rs"
required-features = ["dev-tools"]
```
In the `[package]` section add `default-run = "maple-exp-tracker"` (so `tauri dev` and `cargo run` pick the app, not the dev tool).

- [ ] **Step 5: Configure the app**

```bash
python - <<'EOF'
import json
p = "src-tauri/tauri.conf.json"
c = json.load(open(p, encoding="utf-8"))
c["productName"] = "Maple EXP Tracker"
c["identifier"] = "com.mapleexp.tracker"
w = c["app"]["windows"][0]
w.update({"title": "Maple EXP Tracker", "width": 1100, "height": 760})
c["bundle"]["targets"] = ["nsis"]
json.dump(c, open(p, "w", encoding="utf-8"), indent=2)
EOF
```
In `tsconfig.json` `compilerOptions`, make sure `"strict": true` and add `"noUncheckedIndexedAccess": true`.

- [ ] **Step 6: Add frontend dependencies**

```bash
pnpm add recharts@3.10.1 zod@4.6.7
pnpm add -D vitest@5.0.5
```
Add `"test": "vitest run"` to `package.json` scripts. If pnpm reports a Vite peer mismatch for Vitest 5, upgrade Vite to the version Vitest asks for (`pnpm add -D vite@<required>`), and re-run.

- [ ] **Step 7: Verify both halves build**

```bash
cargo check --manifest-path src-tauri/Cargo.toml
pnpm build
```
Expected: both succeed with no errors.

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "chore: scaffold Tauri 2 react-ts app with tracker dependencies

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: Geometry and the EXP reader

**Files:**
- Create: `src-tauri/src/geom.rs`, `src-tauri/src/read/mod.rs`, `src-tauri/src/read/glyphs.rs`, `src-tauri/src/read/exp.rs`
- Modify: `src-tauri/src/lib.rs` (module declarations)
- Test: inline `#[cfg(test)]` modules; fixture `src-tauri/tests/fixtures/exp_326_0.10.png`

**Interfaces:**
- Produces:
  - `geom::Rect { x: u32, y: u32, w: u32, h: u32 }` + `fits_in(&self, width, height) -> bool`
  - `geom::Calibration { exp: Rect, map: Rect }`
  - `geom::Crop { width, height, rgba: Vec<u8> }` + `new(w, h, rgba) -> Option<Crop>`, `px(x, y) -> [u8; 4]`, `sub(Rect) -> Option<Crop>`, `to_image() -> image::RgbaImage`, `from_image(&RgbaImage) -> Crop`
  - `geom::resolution_key(width, height) -> String` (`"1920x1080"`)
  - `geom::fixture(name) -> Crop` (`#[cfg(test)]`)
  - `read::ReadError` (`Blank`, `UnknownGlyph(u32)`, `Parse(String)`)
  - `read::exp::ExpReading { exp: u64, pct_bp: u32 }` (Serialize, camelCase)
  - `read::exp::Glyph { x: u32, rows: Vec<String> }`, `segment(&Crop) -> Result<Vec<Glyph>, ReadError>`, `classify(&Glyph, &[Template]) -> Option<char>`, `parse(&str) -> Result<ExpReading, ReadError>`, `read_exp(&Crop) -> Result<ExpReading, ReadError>`
  - `read::glyphs::Template { ch: char, rows: &'static [&'static str] }`, `TEMPLATES: &[Template]`

- [ ] **Step 1: Declare modules**

Replace `src-tauri/src/lib.rs` module section (keep `run()` from Task 1 below it):
```rust
pub mod geom;
pub mod read;
```

Create `src-tauri/src/read/mod.rs`:
```rust
//! Pure screen readers: in-memory crop in, parsed value out.

pub mod exp;
pub mod glyphs;

/// A failed read is a normal outcome (loading screen, alt-tab, covered region).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReadError {
    #[error("region contains no text")]
    Blank,
    #[error("unrecognized glyph at column {0}")]
    UnknownGlyph(u32),
    #[error("text {0:?} is not an EXP readout")]
    Parse(String),
}
```

- [ ] **Step 2: Write geometry with its tests**

Create `src-tauri/src/geom.rs`:
```rust
//! Pixel geometry shared by capture, readers and calibration. Crops live only in memory.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    /// True when the rect is non-empty and lies entirely inside a `width`×`height` image.
    pub fn fits_in(&self, width: u32, height: u32) -> bool {
        self.w > 0
            && self.h > 0
            && self.x.checked_add(self.w).is_some_and(|right| right <= width)
            && self.y.checked_add(self.h).is_some_and(|bottom| bottom <= height)
    }
}

/// Calibrated regions for one monitor resolution.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Calibration {
    pub exp: Rect,
    pub map: Rect,
}

pub fn resolution_key(width: u32, height: u32) -> String {
    format!("{width}x{height}")
}

/// Owned RGBA8 pixels, row-major, no row padding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Crop {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Crop {
    /// `None` when `rgba` doesn't hold exactly `width * height` RGBA pixels.
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Option<Self> {
        let expected = (width as usize).checked_mul(height as usize)?.checked_mul(4)?;
        (rgba.len() == expected).then_some(Self { width, height, rgba })
    }

    /// Pixel at (x, y). Callers iterate within `width`/`height`; out of bounds is a programmer error.
    pub fn px(&self, x: u32, y: u32) -> [u8; 4] {
        assert!(x < self.width && y < self.height, "pixel ({x},{y}) outside {}x{}", self.width, self.height);
        let i = (y as usize * self.width as usize + x as usize) * 4;
        [self.rgba[i], self.rgba[i + 1], self.rgba[i + 2], self.rgba[i + 3]]
    }

    /// Copy of the sub-rectangle, or `None` when it doesn't fit.
    pub fn sub(&self, r: Rect) -> Option<Crop> {
        if !r.fits_in(self.width, self.height) {
            return None;
        }
        let mut rgba = Vec::with_capacity(r.w as usize * r.h as usize * 4);
        for y in r.y..r.y + r.h {
            let start = (y as usize * self.width as usize + r.x as usize) * 4;
            rgba.extend_from_slice(&self.rgba[start..start + r.w as usize * 4]);
        }
        Some(Crop { width: r.w, height: r.h, rgba })
    }

    pub fn to_image(&self) -> image::RgbaImage {
        image::RgbaImage::from_raw(self.width, self.height, self.rgba.clone())
            .expect("Crop invariant: rgba length matches dimensions")
    }

    pub fn from_image(img: &image::RgbaImage) -> Self {
        Self { width: img.width(), height: img.height(), rgba: img.as_raw().clone() }
    }
}

#[cfg(test)]
pub fn fixture(name: &str) -> Crop {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
    let img = image::open(&path)
        .unwrap_or_else(|e| panic!("fixture {}: {e}", path.display()))
        .to_rgba8();
    Crop::from_image(&img)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fits_in_rejects_empty_and_overflowing_rects() {
        assert!(Rect { x: 0, y: 0, w: 10, h: 10 }.fits_in(10, 10));
        assert!(!Rect { x: 1, y: 0, w: 10, h: 10 }.fits_in(10, 10));
        assert!(!Rect { x: 0, y: 0, w: 0, h: 10 }.fits_in(10, 10));
        assert!(!Rect { x: u32::MAX, y: 0, w: 2, h: 1 }.fits_in(10, 10));
    }

    #[test]
    fn new_rejects_wrong_buffer_length() {
        assert!(Crop::new(2, 2, vec![0; 16]).is_some());
        assert!(Crop::new(2, 2, vec![0; 15]).is_none());
    }

    #[test]
    fn sub_copies_the_right_pixels() {
        // 3x2 image where each pixel's red channel is its index.
        let rgba: Vec<u8> = (0..6u8).flat_map(|i| [i, 0, 0, 255]).collect();
        let crop = Crop::new(3, 2, rgba).expect("valid buffer");
        let sub = crop.sub(Rect { x: 1, y: 0, w: 2, h: 2 }).expect("fits");
        assert_eq!((sub.width, sub.height), (2, 2));
        assert_eq!([sub.px(0, 0)[0], sub.px(1, 0)[0], sub.px(0, 1)[0], sub.px(1, 1)[0]], [1, 2, 4, 5]);
        assert!(crop.sub(Rect { x: 2, y: 0, w: 2, h: 1 }).is_none());
    }

    #[test]
    fn resolution_key_format() {
        assert_eq!(resolution_key(1920, 1080), "1920x1080");
    }
}
```

- [ ] **Step 3: Run geometry tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml geom::`
Expected: compile error, because `read::exp` and `read::glyphs` don't exist yet. Create empty `src-tauri/src/read/exp.rs` and `src-tauri/src/read/glyphs.rs` files, then re-run.
Expected: 4 passed.

- [ ] **Step 4: Write the templates from the fixture**

These were cut from `exp_326_0.10.png` (`326 [0.10%]`) with luminance ≥ 150 and line trim. The font is drawn at sub-pixel offsets, so the same character can have several shapes (note the two `0`s). Task 9 adds the missing digits. Create `src-tauri/src/read/glyphs.rs`:

```rust
//! EXP-font glyph templates as ASCII art ('#' = text pixel). Several variants per
//! character are expected: the game positions glyphs at sub-pixel offsets.
//! New variants come from the dev-only `glyph-collect` tool.

pub struct Template {
    pub ch: char,
    pub rows: &'static [&'static str],
}

pub const TEMPLATES: &[Template] = &[
    Template { ch: '3', rows: &[
        "..###..",
        ".#####.",
        "#....##",
        "#....##",
        ".....##",
        "...##..",
        ".....##",
        ".....##",
        "#....##",
        ".####..",
        "..###..",
    ] },
    Template { ch: '2', rows: &[
        "..###..",
        ".#####.",
        "##....#",
        "......#",
        "......#",
        "....##.",
        "...##..",
        "...#...",
        ".##....",
        "#######",
        ".######",
    ] },
    Template { ch: '6', rows: &[
        "...##..",
        ".####..",
        "#....##",
        "#......",
        "#......",
        "#####..",
        "#....#.",
        "#....##",
        "#....##",
        ".####..",
        "..###..",
    ] },
    Template { ch: '[', rows: &[
        "..",
        "##",
        "#.",
        "#.",
        "#.",
        "#.",
        "#.",
        "#.",
        "#.",
        "##",
        "..",
    ] },
    Template { ch: '0', rows: &[
        "...##...",
        "..####..",
        ".#....##",
        "##....##",
        "##....##",
        "##....##",
        "##....##",
        "##....##",
        ".#....##",
        "..####..",
        "...##...",
    ] },
    Template { ch: '.', rows: &[
        "..",
        "..",
        "..",
        "..",
        "..",
        "..",
        "..",
        "..",
        "..",
        "##",
        "..",
    ] },
    Template { ch: '1', rows: &[
        "...",
        ".##",
        "###",
        ".##",
        ".##",
        ".##",
        ".##",
        ".##",
        ".##",
        ".##",
        "...",
    ] },
    Template { ch: '0', rows: &[
        "..#.#..",
        ".#####.",
        "##...##",
        "#.....#",
        "#.....#",
        "#.....#",
        "#.....#",
        "#.....#",
        "#.....#",
        ".#####.",
        "..###..",
    ] },
    Template { ch: '%', rows: &[
        "..........",
        "..#......#",
        "##.##..##.",
        ".###..##..",
        "..#...#...",
        ".....#....",
        "...##...#.",
        "...#...##.",
        "..#...##.#",
        ".#.....##.",
        "..........",
    ] },
    Template { ch: ']', rows: &[
        ".",
        "#",
        ".",
        ".",
        ".",
        ".",
        ".",
        ".",
        ".",
        "#",
        ".",
    ] },
];
```

- [ ] **Step 5: Write the failing EXP reader tests**

Create `src-tauri/src/read/exp.rs` with only the tests:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::{fixture, Crop};
    use crate::read::glyphs::TEMPLATES;

    #[test]
    fn reads_the_fixture() {
        let reading = read_exp(&fixture("exp_326_0.10.png")).expect("fixture is readable");
        assert_eq!(reading, ExpReading { exp: 326, pct_bp: 10 });
    }

    #[test]
    fn segments_fixture_into_ten_glyphs() {
        let glyphs = segment(&fixture("exp_326_0.10.png")).expect("text present");
        let text: String = glyphs.iter().map(|g| classify(g, TEMPLATES).unwrap_or('?')).collect();
        assert_eq!(text, "326[0.10%]");
    }

    #[test]
    fn blank_crop_is_blank() {
        let dark = Crop::new(20, 10, [30u8, 30, 30, 255].repeat(200)).expect("valid buffer");
        assert_eq!(read_exp(&dark), Err(ReadError::Blank));
    }

    #[test]
    fn classify_tolerates_a_shifted_variant() {
        // The 8-wide '0' with one pixel flipped still classifies as '0'.
        let mut rows: Vec<String> = TEMPLATES.iter().find(|t| t.ch == '0').expect("has 0").rows.iter().map(|r| r.to_string()).collect();
        rows[0] = "..###...".into();
        assert_eq!(classify(&Glyph { x: 0, rows }, TEMPLATES), Some('0'));
    }

    #[test]
    fn classify_rejects_garbage() {
        let rows = vec!["##########".to_string(); 11];
        assert_eq!(classify(&Glyph { x: 0, rows }, TEMPLATES), None);
    }

    #[test]
    fn parse_accepts_readouts() {
        assert_eq!(parse("326[0.10%]"), Ok(ExpReading { exp: 326, pct_bp: 10 }));
        assert_eq!(parse("1,234,567[45.67%]"), Ok(ExpReading { exp: 1_234_567, pct_bp: 4567 }));
        assert_eq!(parse("0[100.00%]"), Ok(ExpReading { exp: 0, pct_bp: 10_000 }));
    }

    #[test]
    fn parse_rejects_malformed_text() {
        for bad in ["326[0.1%]", "[0.10%]", "326[0.10%", "326[0.10]", "32a[0.10%]", "326[100.01%]", ""] {
            assert!(matches!(parse(bad), Err(ReadError::Parse(_))), "{bad:?} should fail");
        }
    }
}
```

- [ ] **Step 6: Run to verify they fail**

Run: `cargo test --manifest-path src-tauri/Cargo.toml read::exp`
Expected: FAIL to compile (`cannot find function read_exp`, `segment`, `classify`, `parse`, type `ExpReading`, `Glyph`).

- [ ] **Step 7: Implement the reader**

Prepend to `src-tauri/src/read/exp.rs` (above the tests):
```rust
//! EXP readout reader: `326 [0.10%]` → ExpReading. Template matching, not OCR:
//! Windows OCR misreads this font (`[0.10%]` → `to. 10%)`, verified 2026-10-15).

use serde::Serialize;

use super::glyphs::{Template, TEMPLATES};
use super::ReadError;
use crate::geom::Crop;

/// Text pixels have luminance ≥ this (measured on the 2026-10-15 fixture).
pub const TEXT_LUM_MIN: u32 = 150;
/// Max share of differing pixels for a glyph to match a template.
const MAX_MISMATCH_RATIO: f32 = 0.15;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpReading {
    pub exp: u64,
    /// Percent in basis points: 0.10% → 10.
    pub pct_bp: u32,
}

/// One glyph cut from the text line: rows of '#'/'.', and its left column in the crop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Glyph {
    pub x: u32,
    pub rows: Vec<String>,
}

fn is_text(px: [u8; 4]) -> bool {
    let [r, g, b, _] = px.map(u32::from);
    (299 * r + 587 * g + 114 * b) / 1000 >= TEXT_LUM_MIN
}

/// Trim the line to its text rows, then split at empty columns.
pub fn segment(crop: &Crop) -> Result<Vec<Glyph>, ReadError> {
    let (w, h) = (crop.width, crop.height);
    let mask: Vec<bool> = (0..h).flat_map(|y| (0..w).map(move |x| (x, y))).map(|(x, y)| is_text(crop.px(x, y))).collect();
    let at = |x: u32, y: u32| mask[(y * w + x) as usize];
    let text_rows: Vec<u32> = (0..h).filter(|&y| (0..w).any(|x| at(x, y))).collect();
    let (Some(&top), Some(&bottom)) = (text_rows.first(), text_rows.last()) else {
        return Err(ReadError::Blank);
    };
    let column_has_text = |x: u32| (top..=bottom).any(|y| at(x, y));

    let mut glyphs = Vec::new();
    let mut x = 0;
    while x < w {
        if !column_has_text(x) {
            x += 1;
            continue;
        }
        let start = x;
        while x < w && column_has_text(x) {
            x += 1;
        }
        let rows = (top..=bottom)
            .map(|y| (start..x).map(|gx| if at(gx, y) { '#' } else { '.' }).collect())
            .collect();
        glyphs.push(Glyph { x: start, rows });
    }
    Ok(glyphs)
}

fn cell(rows: &[&[u8]], col: usize, row: usize) -> bool {
    rows.get(row).and_then(|r| r.get(col)).is_some_and(|&b| b == b'#')
}

/// Differing pixels between two glyphs, top-aligned, trying both horizontal
/// alignments when widths differ by one. `None` when sizes are too different.
fn mismatches(a: &[String], b: &[&str]) -> Option<(usize, usize)> {
    let a: Vec<&[u8]> = a.iter().map(|r| r.as_bytes()).collect();
    let b: Vec<&[u8]> = b.iter().map(|r| r.as_bytes()).collect();
    let (wa, wb) = (a.first()?.len(), b.first()?.len());
    if wa.abs_diff(wb) > 1 || a.len().abs_diff(b.len()) > 2 {
        return None;
    }
    let (width, height) = (wa.max(wb), a.len().max(b.len()));
    let shifts: &[(usize, usize)] = if wa == wb { &[(0, 0)] } else if wa < wb { &[(0, 0), (1, 0)] } else { &[(0, 0), (0, 1)] };
    shifts
        .iter()
        .map(|&(shift_a, shift_b)| {
            (0..height)
                .flat_map(|row| (0..width).map(move |col| (row, col)))
                .filter(|&(row, col)| {
                    let pa = col.checked_sub(shift_a).is_some_and(|c| cell(&a, c, row));
                    let pb = col.checked_sub(shift_b).is_some_and(|c| cell(&b, c, row));
                    pa != pb
                })
                .count()
        })
        .min()
        .map(|n| (n, width * height))
}

/// Nearest template character, if it is close enough and strictly better than
/// the best template of any other character.
pub fn classify(glyph: &Glyph, templates: &[Template]) -> Option<char> {
    let mut best_per_char: Vec<(char, usize, usize)> = Vec::new();
    for t in templates {
        let Some((n, area)) = mismatches(&glyph.rows, t.rows) else { continue };
        match best_per_char.iter_mut().find(|(c, _, _)| *c == t.ch) {
            Some(entry) if n < entry.1 => *entry = (t.ch, n, area),
            Some(_) => {}
            None => best_per_char.push((t.ch, n, area)),
        }
    }
    best_per_char.sort_by_key(|&(_, n, _)| n);
    let &(ch, n, area) = best_per_char.first()?;
    let close_enough = (n as f32) <= MAX_MISMATCH_RATIO * area as f32;
    let unambiguous = best_per_char.get(1).is_none_or(|&(_, second, _)| second > n);
    (close_enough && unambiguous).then_some(ch)
}

/// Parse `<digits/commas>[<int>.<2 digits>%]`.
pub fn parse(text: &str) -> Result<ExpReading, ReadError> {
    let bad = || ReadError::Parse(text.to_string());
    let (number, rest) = text.split_once('[').ok_or_else(bad)?;
    let pct = rest.strip_suffix("%]").ok_or_else(bad)?;
    let (whole, frac) = pct.split_once('.').ok_or_else(bad)?;
    let digits: String = number.chars().filter(|&c| c != ',').collect();
    let all_digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if !all_digits(&digits) || !all_digits(whole) || frac.len() != 2 || !all_digits(frac) {
        return Err(bad());
    }
    let exp = digits.parse::<u64>().map_err(|_| bad())?;
    let whole = whole.parse::<u32>().map_err(|_| bad())?;
    let frac = frac.parse::<u32>().map_err(|_| bad())?;
    let pct_bp = whole.checked_mul(100).and_then(|v| v.checked_add(frac)).ok_or_else(bad)?;
    if pct_bp > 10_000 {
        return Err(bad());
    }
    Ok(ExpReading { exp, pct_bp })
}

pub fn read_exp(crop: &Crop) -> Result<ExpReading, ReadError> {
    let text = segment(crop)?
        .iter()
        .map(|g| classify(g, TEMPLATES).ok_or(ReadError::UnknownGlyph(g.x)))
        .collect::<Result<String, _>>()?;
    parse(&text)
}
```

- [ ] **Step 8: Run to verify they pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml read::exp`
Expected: 7 passed. If `segments_fixture_into_ten_glyphs` fails, print the glyphs (`dbg!(&glyphs)`) and compare against the templates. Don't loosen `MAX_MISMATCH_RATIO` to force a pass.

- [ ] **Step 9: Commit**

```bash
git add src-tauri/src
git commit -m "feat: geometry types and template-based EXP reader

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: Map strip reader and map confirmation

**Files:**
- Create: `src-tauri/src/read/map_strip.rs`, `src-tauri/src/tracker/mod.rs` (module skeleton only), `src-tauri/src/tracker/confirm.rs`
- Modify: `src-tauri/src/read/mod.rs`, `src-tauri/src/lib.rs`
- Test: inline; fixture `map_victoria_road_perion.png`

**Interfaces:**
- Consumes: `geom::Crop`, `read::ReadError`
- Produces:
  - `read::map_strip::Signature(pub Vec<u8>)` + `matches(&self, &Signature) -> bool`
  - `read::map_strip::StripRead { signature: Signature }`, `read_strip(&Crop) -> Result<StripRead, ReadError>`
  - `tracker::MapConfirmer::new()`, `feed(&mut self, Signature, Crop) -> ConfirmOutcome`
  - `tracker::ConfirmOutcome::{Unchanged, Changed { signature: Signature, strips: Vec<Crop> }}`, `tracker::CONFIRM_FRAMES = 3`

- [ ] **Step 1: Declare modules**

In `src-tauri/src/read/mod.rs` add `pub mod map_strip;`. In `src-tauri/src/lib.rs` add `pub mod tracker;`. Create `src-tauri/src/tracker/mod.rs`:
```rust
//! Pure EXP/session state machine and map debouncing. No I/O.

mod confirm;

pub use confirm::{ConfirmOutcome, MapConfirmer, CONFIRM_FRAMES};
```

- [ ] **Step 2: Write the failing strip tests**

Create `src-tauri/src/read/map_strip.rs` with only:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::{fixture, Crop};

    #[test]
    fn fixture_signature_matches_measurement() {
        let read = read_strip(&fixture("map_victoria_road_perion.png")).expect("readable");
        // Measured with the reference script on 2026-10-15: background run starts at
        // column 2, minimap edge at column 237, 514 text pixels.
        assert_eq!(read.signature.0.len(), 235);
        assert_eq!(read.signature.0.iter().map(|&c| u32::from(c)).sum::<u32>(), 514);
    }

    #[test]
    fn signature_matches_itself_but_not_a_shift() {
        let sig = read_strip(&fixture("map_victoria_road_perion.png")).expect("readable").signature;
        assert!(sig.matches(&sig));
        let mut shifted = sig.0.clone();
        shifted.rotate_right(3);
        assert!(!sig.matches(&Signature(shifted)));
        let mut nudged = sig.0.clone();
        nudged[10] = nudged[10].saturating_add(4);
        assert!(sig.matches(&Signature(nudged)));
        assert!(!sig.matches(&Signature(sig.0[..sig.0.len() - 1].to_vec())));
    }

    #[test]
    fn background_without_text_is_blank() {
        let empty = Crop::new(40, 10, [153u8, 187, 204, 255].repeat(400)).expect("valid buffer");
        assert_eq!(read_strip(&empty).map(|r| r.signature), Err(ReadError::Blank));
    }

    #[test]
    fn loading_screen_is_blank() {
        let black = Crop::new(40, 10, [0u8, 0, 0, 255].repeat(400)).expect("valid buffer");
        assert_eq!(read_strip(&black).map(|r| r.signature), Err(ReadError::Blank));
    }
}
```

- [ ] **Step 3: Run to verify they fail**

Run: `cargo test --manifest-path src-tauri/Cargo.toml read::map_strip`
Expected: FAIL to compile (`read_strip`, `Signature` not found).

- [ ] **Step 4: Implement the strip reader**

Prepend to `src-tauri/src/read/map_strip.rs`:
```rust
//! Minimap name strip → pixel signature. The signature, not OCR, identifies a map:
//! the game draws a map's name identically every frame, while Windows OCR output
//! varies with upscale factor (verified 2026-10-15).

use super::ReadError;
use crate::geom::Crop;

/// Flat background of the name strip (measured 2026-10-15).
pub const STRIP_BG: [u8; 3] = [153, 187, 204];
const BG_TOLERANCE: u8 = 6;
/// Text pixels: every channel ≥ this.
const TEXT_MIN_CHANNEL: u8 = 215;
/// A row above the text, used to find where the background starts and the minimap edge.
const EDGE_PROBE_ROW: u32 = 2;
/// Summed per-column difference still considered the same map.
const MATCH_TOLERANCE: u32 = 4;

/// Text-pixel count per column, from the start of the background to the minimap's right edge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signature(pub Vec<u8>);

impl Signature {
    pub fn matches(&self, other: &Signature) -> bool {
        self.0.len() == other.0.len()
            && self.0.iter().zip(&other.0).map(|(a, b)| u32::from(a.abs_diff(*b))).sum::<u32>() <= MATCH_TOLERANCE
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StripRead {
    pub signature: Signature,
}

pub fn read_strip(crop: &Crop) -> Result<StripRead, ReadError> {
    if crop.height <= EDGE_PROBE_ROW {
        return Err(ReadError::Blank);
    }
    let is_bg = |x: u32| {
        let p = crop.px(x, EDGE_PROBE_ROW);
        (0..3).all(|i| p[i].abs_diff(STRIP_BG[i]) <= BG_TOLERANCE)
    };
    let start = (0..crop.width).find(|&x| is_bg(x)).ok_or(ReadError::Blank)?;
    let end = (start..crop.width).find(|&x| !is_bg(x)).unwrap_or(crop.width);
    let counts: Vec<u8> = (start..end)
        .map(|x| {
            let n = (0..crop.height)
                .filter(|&y| {
                    let p = crop.px(x, y);
                    p[0].min(p[1]).min(p[2]) >= TEXT_MIN_CHANNEL
                })
                .count();
            u8::try_from(n).unwrap_or(u8::MAX)
        })
        .collect();
    if counts.iter().all(|&c| c == 0) {
        return Err(ReadError::Blank);
    }
    Ok(StripRead { signature: Signature(counts) })
}
```

- [ ] **Step 5: Run strip tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml read::map_strip`
Expected: 4 passed.

- [ ] **Step 6: Write the failing confirmer tests**

Create `src-tauri/src/tracker/confirm.rs` with only:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn sig(v: u8) -> Signature {
        Signature(vec![v; 8])
    }
    fn strip() -> Crop {
        Crop::new(1, 1, vec![0; 4]).expect("valid buffer")
    }

    #[test]
    fn confirms_after_three_matching_frames() {
        let mut c = MapConfirmer::new();
        assert_eq!(c.feed(sig(10), strip()), ConfirmOutcome::Unchanged);
        assert_eq!(c.feed(sig(10), strip()), ConfirmOutcome::Unchanged);
        match c.feed(sig(10), strip()) {
            ConfirmOutcome::Changed { signature, strips } => {
                assert_eq!(signature, sig(10));
                assert_eq!(strips.len(), CONFIRM_FRAMES);
            }
            other => panic!("expected Changed, got {other:?}"),
        }
        assert_eq!(c.feed(sig(10), strip()), ConfirmOutcome::Unchanged);
    }

    #[test]
    fn a_different_frame_restarts_the_streak() {
        let mut c = MapConfirmer::new();
        c.feed(sig(10), strip());
        c.feed(sig(10), strip());
        c.feed(sig(50), strip());
        c.feed(sig(10), strip());
        c.feed(sig(10), strip());
        assert!(matches!(c.feed(sig(10), strip()), ConfirmOutcome::Changed { .. }));
    }

    #[test]
    fn flicker_back_to_confirmed_map_cancels_candidate() {
        let mut c = MapConfirmer::new();
        for _ in 0..3 {
            c.feed(sig(10), strip());
        }
        c.feed(sig(50), strip());
        c.feed(sig(50), strip());
        assert_eq!(c.feed(sig(10), strip()), ConfirmOutcome::Unchanged);
        assert_eq!(c.feed(sig(50), strip()), ConfirmOutcome::Unchanged);
    }
}
```

- [ ] **Step 7: Run to verify they fail**

Run: `cargo test --manifest-path src-tauri/Cargo.toml tracker::confirm`
Expected: FAIL to compile.

- [ ] **Step 8: Implement the confirmer**

Prepend to `src-tauri/src/tracker/confirm.rs`:
```rust
//! Debounces map changes: a signature counts once seen on CONFIRM_FRAMES consecutive
//! readable frames. Callers must not feed unreadable frames — those neither confirm
//! nor break a streak.

use crate::geom::Crop;
use crate::read::map_strip::Signature;

pub const CONFIRM_FRAMES: usize = 3;

#[derive(Debug, PartialEq, Eq)]
pub enum ConfirmOutcome {
    Unchanged,
    /// A new map was confirmed; `strips` are the confirming crops (for OCR labelling).
    Changed { signature: Signature, strips: Vec<Crop> },
}

#[derive(Debug, Default)]
pub struct MapConfirmer {
    confirmed: Option<Signature>,
    candidate: Option<(Signature, Vec<Crop>)>,
}

impl MapConfirmer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn feed(&mut self, signature: Signature, strip: Crop) -> ConfirmOutcome {
        if self.confirmed.as_ref().is_some_and(|c| c.matches(&signature)) {
            self.candidate = None;
            return ConfirmOutcome::Unchanged;
        }
        let (sig, strips) = match self.candidate.take() {
            Some((sig, mut strips)) if sig.matches(&signature) => {
                strips.push(strip);
                (sig, strips)
            }
            _ => (signature, vec![strip]),
        };
        if strips.len() >= CONFIRM_FRAMES {
            self.confirmed = Some(sig.clone());
            return ConfirmOutcome::Changed { signature: sig, strips };
        }
        self.candidate = Some((sig, strips));
        ConfirmOutcome::Unchanged
    }
}
```

- [ ] **Step 9: Run tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml tracker::confirm read::map_strip`
Expected: all pass (`cargo test` accepts one filter. Run `cargo test --manifest-path src-tauri/Cargo.toml` to run everything).

- [ ] **Step 10: Commit**

```bash
git add src-tauri/src
git commit -m "feat: minimap strip signature reader and map confirmation debounce

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: Map label OCR

**Files:**
- Create: `src-tauri/src/read/ocr.rs`
- Modify: `src-tauri/src/read/mod.rs`

**Interfaces:**
- Consumes: `geom::Crop` (`to_image`)
- Produces: `read::ocr::MapOcr::new() -> Result<MapOcr, OcrError>`, `label(&self, &[Crop]) -> Result<Option<String>, OcrError>`, `normalize(&str) -> String`, `title_case(&str) -> String`, `OcrError`

- [ ] **Step 1: Declare the module**

In `src-tauri/src/read/mod.rs` add `pub mod ocr;`.

- [ ] **Step 2: Write the failing tests**

Create `src-tauri/src/read/ocr.rs` with only:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::fixture;

    #[test]
    fn normalize_collapses_whitespace_and_case() {
        assert_eq!(normalize("  Victoria   Road\nperion "), "victoria road perion");
    }

    #[test]
    fn title_case_capitalizes_words() {
        assert_eq!(title_case("victoria road perion"), "Victoria Road Perion");
        assert_eq!(title_case(""), "");
    }

    /// Real Windows OCR. Verified 2026-10-15: all four variants read this strip as
    /// "victoria road perion" after normalization.
    #[test]
    fn labels_the_fixture() {
        let ocr = MapOcr::new().expect("Windows OCR available");
        let label = ocr.label(&[fixture("map_victoria_road_perion.png")]).expect("OCR runs");
        assert_eq!(label.as_deref(), Some("Victoria Road Perion"));
    }
}
```

- [ ] **Step 3: Run to verify they fail**

Run: `cargo test --manifest-path src-tauri/Cargo.toml read::ocr`
Expected: FAIL to compile.

- [ ] **Step 4: Implement**

Prepend to `src-tauri/src/read/ocr.rs`:
```rust
//! Windows built-in OCR, used only to *label* a newly confirmed map. Identity is the
//! pixel signature. Single OCR runs are unreliable on this font, so several upscale
//! variants across the confirming frames vote.

use std::collections::HashMap;

use image::imageops::{resize, FilterType};
use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::DataWriter;

use crate::geom::Crop;

const VARIANTS: [(u32, FilterType); 4] = [
    (2, FilterType::Nearest),
    (3, FilterType::Nearest),
    (3, FilterType::CatmullRom),
    (3, FilterType::Triangle),
];

#[derive(Debug, thiserror::Error)]
pub enum OcrError {
    #[error("Windows OCR: {0}")]
    Windows(#[from] windows::core::Error),
    #[error("image too large for OCR")]
    TooLarge,
}

pub struct MapOcr {
    engine: OcrEngine,
}

impl MapOcr {
    pub fn new() -> Result<Self, OcrError> {
        Ok(Self { engine: OcrEngine::TryCreateFromUserProfileLanguages()? })
    }

    /// Most-voted normalized text across all strips and variants, title-cased.
    /// `None` when OCR returns no text at all.
    pub fn label(&self, strips: &[Crop]) -> Result<Option<String>, OcrError> {
        let mut votes: HashMap<String, usize> = HashMap::new();
        for strip in strips {
            let img = strip.to_image();
            for (scale, filter) in VARIANTS {
                let up = resize(&img, img.width() * scale, img.height() * scale, filter);
                let text = normalize(&self.recognize(&up)?);
                if !text.is_empty() {
                    *votes.entry(text).or_default() += 1;
                }
            }
        }
        Ok(votes
            .into_iter()
            .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(&a.0)))
            .map(|(text, _)| title_case(&text)))
    }

    fn recognize(&self, img: &image::RgbaImage) -> Result<String, OcrError> {
        let mut bgra = img.as_raw().clone();
        for px in bgra.chunks_exact_mut(4) {
            px.swap(0, 2);
        }
        let writer = DataWriter::new()?;
        writer.WriteBytes(&bgra)?;
        let width = i32::try_from(img.width()).map_err(|_| OcrError::TooLarge)?;
        let height = i32::try_from(img.height()).map_err(|_| OcrError::TooLarge)?;
        let bitmap = SoftwareBitmap::CreateCopyFromBuffer(&writer.DetachBuffer()?, BitmapPixelFormat::Bgra8, width, height)?;
        Ok(self.engine.RecognizeAsync(&bitmap)?.join()?.Text()?.to_string_lossy())
    }
}

pub fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

pub fn title_case(text: &str) -> String {
    text.split(' ')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml read::ocr`
Expected: 3 passed.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/read
git commit -m "feat: voted Windows OCR for map labels

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: Tracker (level bounds, gains, sessions, training time, minute rollup)

**Files:**
- Create: `src-tauri/src/tracker/level.rs`
- Modify: `src-tauri/src/tracker/mod.rs`

**Interfaces:**
- Consumes: `read::exp::ExpReading`
- Produces:
  - `tracker::TotalBounds { lo: u64, hi: u64 }`: `from_reading(ExpReading) -> Option<Self>`, `intersect(self, Self) -> Option<Self>`, `estimate(self) -> u64`
  - `tracker::MapId = i64`, `tracker::SessionKey(pub u64)`
  - `tracker::Sample { t: i64, exp: Option<ExpReading>, map: Option<MapId> }`
  - `tracker::Effect::{SessionStarted { key, map, t }, SessionEnded { key, t }, Minute { minute_start, key, exp_gained: u64, training_secs: u32 }}`
  - `tracker::Tracker::new()`, `on_sample(&mut self, Sample) -> Vec<Effect>`, `finish(&mut self) -> Vec<Effect>`, `baseline() -> Option<ExpReading>`, `total_estimate() -> Option<u64>`

- [ ] **Step 1: Write failing bounds tests**

Create `src-tauri/src/tracker/level.rs` with only:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn r(exp: u64, pct_bp: u32) -> ExpReading {
        ExpReading { exp, pct_bp }
    }

    #[test]
    fn high_percent_gives_tight_bounds_around_true_total() {
        let b = TotalBounds::from_reading(r(990_000, 9_900)).expect("informative");
        assert_eq!(b, TotalBounds { lo: 999_899, hi: 1_000_102 });
        assert_eq!(b.estimate(), 1_000_000);
    }

    #[test]
    fn too_little_information_gives_none() {
        assert_eq!(TotalBounds::from_reading(r(5, 1)), None);
        assert_eq!(TotalBounds::from_reading(r(0, 500)), None);
    }

    #[test]
    fn intersect_narrows_or_detects_conflict() {
        let a = TotalBounds { lo: 10, hi: 20 };
        assert_eq!(a.intersect(TotalBounds { lo: 15, hi: 30 }), Some(TotalBounds { lo: 15, hi: 20 }));
        assert_eq!(a.intersect(TotalBounds { lo: 21, hi: 30 }), None);
    }
}
```
In `src-tauri/src/tracker/mod.rs` add `mod level;` and `pub use level::TotalBounds;`.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --manifest-path src-tauri/Cargo.toml tracker::level`
Expected: FAIL to compile.

- [ ] **Step 3: Implement bounds**

Prepend to `src-tauri/src/tracker/level.rs`:
```rust
//! The level's total EXP as an interval. The readout's percent is rounded or
//! truncated to 0.01%, so one reading only bounds the total; intersecting the
//! bounds across a level tightens them and exposes misreads.

use crate::read::exp::ExpReading;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TotalBounds {
    pub lo: u64,
    pub hi: u64,
}

impl TotalBounds {
    /// Total ∈ [exp·10000/(p+1), exp·10000/(p−1)]. `None` when p < 2 or exp = 0.
    pub fn from_reading(r: ExpReading) -> Option<Self> {
        if r.pct_bp < 2 || r.exp == 0 {
            return None;
        }
        let scaled = u128::from(r.exp) * 10_000;
        let lo = scaled / u128::from(r.pct_bp + 1);
        let hi = scaled.div_ceil(u128::from(r.pct_bp - 1));
        Some(Self { lo: u64::try_from(lo).ok()?, hi: u64::try_from(hi).ok()? })
    }

    pub fn intersect(self, other: Self) -> Option<Self> {
        let lo = self.lo.max(other.lo);
        let hi = self.hi.min(other.hi);
        (lo <= hi).then_some(Self { lo, hi })
    }

    pub fn estimate(self) -> u64 {
        self.lo + (self.hi - self.lo) / 2
    }
}
```

- [ ] **Step 4: Run bounds tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml tracker::level`
Expected: 3 passed.

- [ ] **Step 5: Write failing tracker tests**

Append to `src-tauri/src/tracker/mod.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    const MAP: Option<MapId> = Some(1);
    const TOWN: Option<MapId> = Some(2);

    fn reading(exp: u64, pct_bp: u32) -> Option<ExpReading> {
        Some(ExpReading { exp, pct_bp })
    }

    fn feed(tr: &mut Tracker, t: i64, exp: Option<ExpReading>, map: Option<MapId>) -> Vec<Effect> {
        tr.on_sample(Sample { t, exp, map })
    }

    fn minutes(fx: &[Effect]) -> (u64, u32) {
        fx.iter().fold((0, 0), |(e, s), f| match f {
            Effect::Minute { exp_gained, training_secs, .. } => (e + exp_gained, s + training_secs),
            _ => (e, s),
        })
    }

    /// 10_000 + 10t EXP at exactly (1000 + t) bp: consistent with a 100_000 total.
    fn steady(t: i64) -> Option<ExpReading> {
        let t = u64::try_from(t).expect("non-negative t");
        reading(10_000 + 10 * t, 1_000 + u32::try_from(t).expect("small t"))
    }

    #[test]
    fn gain_starts_session_and_rolls_up_per_minute() {
        let mut tr = Tracker::new();
        assert!(feed(&mut tr, 0, reading(100, 100), MAP).is_empty());
        assert_eq!(feed(&mut tr, 1, reading(150, 150), MAP), vec![Effect::SessionStarted { key: SessionKey(0), map: 1, t: 1 }]);
        assert_eq!(
            feed(&mut tr, 60, None, MAP),
            vec![Effect::Minute { minute_start: 0, key: SessionKey(0), exp_gained: 50, training_secs: 1 }]
        );
    }

    #[test]
    fn level_up_counts_the_rest_of_the_old_level() {
        let mut tr = Tracker::new();
        feed(&mut tr, 0, reading(990_000, 9_900), MAP);
        feed(&mut tr, 1, reading(5_000, 50), MAP);
        let fx = tr.finish();
        assert_eq!(minutes(&fx).0, 15_000);
        assert_eq!(tr.baseline(), Some(ExpReading { exp: 5_000, pct_bp: 50 }));
    }

    #[test]
    fn death_resets_baseline_without_negative_gain() {
        let mut tr = Tracker::new();
        feed(&mut tr, 0, reading(500_000, 5_000), MAP);
        feed(&mut tr, 1, reading(450_000, 4_500), MAP);
        feed(&mut tr, 2, reading(460_000, 4_600), MAP);
        assert_eq!(minutes(&tr.finish()).0, 10_000);
    }

    #[test]
    fn inconsistent_reading_is_rejected() {
        let mut tr = Tracker::new();
        feed(&mut tr, 0, reading(500_000, 5_000), MAP);
        feed(&mut tr, 1, reading(900_000, 5_100), MAP);
        assert_eq!(tr.baseline(), Some(ExpReading { exp: 500_000, pct_bp: 5_000 }));
        feed(&mut tr, 2, reading(510_000, 5_100), MAP);
        assert_eq!(minutes(&tr.finish()).0, 10_000);
    }

    #[test]
    fn repeated_rejections_reset_the_baseline() {
        let mut tr = Tracker::new();
        feed(&mut tr, 0, reading(500_000, 5_000), MAP);
        for t in 1..=i64::from(MAX_CONSECUTIVE_REJECTIONS) {
            feed(&mut tr, t, reading(900_000, 5_100), MAP);
        }
        assert_eq!(tr.baseline(), Some(ExpReading { exp: 900_000, pct_bp: 5_100 }));
        let fx = feed(&mut tr, 6, reading(900_100, 5_100), MAP);
        assert_eq!(fx, vec![Effect::SessionStarted { key: SessionKey(0), map: 1, t: 6 }]);
        assert_eq!(minutes(&tr.finish()).0, 100);
    }

    #[test]
    fn gains_without_a_confirmed_map_are_not_recorded() {
        let mut tr = Tracker::new();
        feed(&mut tr, 0, steady(0), None);
        assert!(feed(&mut tr, 1, steady(1), None).is_empty());
        assert!(tr.finish().is_empty());
    }

    #[test]
    fn repot_trip_within_window_keeps_session_and_excludes_town_time() {
        let mut tr = Tracker::new();
        let mut fx = Vec::new();
        for t in 0..=10 {
            fx.extend(feed(&mut tr, t, steady(t), MAP));
        }
        for t in 11..=200 {
            fx.extend(feed(&mut tr, t, None, TOWN));
        }
        fx.extend(feed(&mut tr, 201, steady(20), MAP));
        fx.extend(tr.finish());
        let started = fx.iter().filter(|f| matches!(f, Effect::SessionStarted { .. })).count();
        let ended: Vec<_> = fx.iter().filter(|f| matches!(f, Effect::SessionEnded { .. })).collect();
        assert_eq!(started, 1);
        assert_eq!(ended, vec![&Effect::SessionEnded { key: SessionKey(0), t: 201 }]);
        assert_eq!(minutes(&fx), (200, 11));
    }

    #[test]
    fn away_longer_than_window_ends_session() {
        let mut tr = Tracker::new();
        let mut fx = Vec::new();
        for t in 0..=10 {
            fx.extend(feed(&mut tr, t, steady(t), MAP));
        }
        for t in 11..=700 {
            fx.extend(feed(&mut tr, t, None, TOWN));
        }
        assert!(fx.contains(&Effect::SessionEnded { key: SessionKey(0), t: 10 }));
        let fx = feed(&mut tr, 701, steady(20), MAP);
        assert!(fx.contains(&Effect::SessionStarted { key: SessionKey(1), map: 1, t: 701 }));
    }

    #[test]
    fn gain_on_another_map_ends_the_session() {
        let mut tr = Tracker::new();
        feed(&mut tr, 0, steady(0), MAP);
        feed(&mut tr, 1, steady(1), MAP);
        let fx = feed(&mut tr, 2, steady(2), Some(3));
        assert_eq!(
            fx,
            vec![
                Effect::SessionEnded { key: SessionKey(0), t: 1 },
                Effect::SessionStarted { key: SessionKey(1), map: 3, t: 2 },
            ]
        );
    }

    #[test]
    fn afk_stops_counting_after_grace_period() {
        let mut tr = Tracker::new();
        let mut fx = Vec::new();
        for t in 0..=10 {
            fx.extend(feed(&mut tr, t, steady(t), MAP));
        }
        for t in 11..=300 {
            fx.extend(feed(&mut tr, t, None, MAP));
        }
        fx.extend(tr.finish());
        assert_eq!(minutes(&fx), (100, 130));
    }

    #[test]
    fn capture_gap_is_capped() {
        let mut tr = Tracker::new();
        feed(&mut tr, 0, steady(0), MAP);
        feed(&mut tr, 1, steady(1), MAP);
        feed(&mut tr, 50, steady(2), MAP);
        assert_eq!(minutes(&tr.finish()).1, 1 + MAX_SAMPLE_GAP_SECS as u32);
    }
}
```

- [ ] **Step 6: Run to verify failure**

Run: `cargo test --manifest-path src-tauri/Cargo.toml tracker::tests`
Expected: FAIL to compile (`Tracker`, `Sample`, `Effect`, … not found).

- [ ] **Step 7: Implement the tracker**

Replace the top of `src-tauri/src/tracker/mod.rs` (everything above `#[cfg(test)]`) with:
```rust
//! Pure EXP/session state machine and map debouncing: samples in, effects out. No I/O.

mod confirm;
mod level;

pub use confirm::{ConfirmOutcome, MapConfirmer, CONFIRM_FRAMES};
pub use level::TotalBounds;

use std::collections::BTreeMap;

use crate::read::exp::ExpReading;

pub type MapId = i64;

/// Tracker-local session identity; the pipeline maps it to a database id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SessionKey(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub t: i64,
    pub exp: Option<ExpReading>,
    /// The currently confirmed map, if any.
    pub map: Option<MapId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    SessionStarted { key: SessionKey, map: MapId, t: i64 },
    SessionEnded { key: SessionKey, t: i64 },
    Minute { minute_start: i64, key: SessionKey, exp_gained: u64, training_secs: u32 },
}

pub const TRAINING_GRACE_SECS: i64 = 120;
pub const MAX_SAMPLE_GAP_SECS: i64 = 3;
pub const RETURN_WINDOW_SECS: i64 = 600;
pub const LEVEL_UP_FROM_BP: u32 = 8_000;
pub const LEVEL_UP_TO_BP: u32 = 2_000;
pub const MAX_CONSECUTIVE_REJECTIONS: u32 = 5;

#[derive(Clone, Copy, Debug)]
struct ActiveSession {
    key: SessionKey,
    map: MapId,
    last_on_map: i64,
}

#[derive(Debug, Default)]
pub struct Tracker {
    baseline: Option<ExpReading>,
    bounds: Option<TotalBounds>,
    rejections: u32,
    last_gain_t: Option<i64>,
    last_t: Option<i64>,
    session: Option<ActiveSession>,
    next_key: u64,
    minute: Option<i64>,
    /// Per-session (exp_gained, training_secs) for the current minute.
    acc: BTreeMap<SessionKey, (u64, u32)>,
}

impl Tracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn baseline(&self) -> Option<ExpReading> {
        self.baseline
    }

    pub fn total_estimate(&self) -> Option<u64> {
        self.bounds.map(TotalBounds::estimate)
    }

    pub fn on_sample(&mut self, s: Sample) -> Vec<Effect> {
        let mut fx = Vec::new();
        self.roll_minute(s.t, &mut fx);
        let gain = s.exp.map_or(0, |r| self.accept(r));

        if let Some(sess) = self.session.as_mut() {
            if s.map == Some(sess.map) {
                sess.last_on_map = s.t;
            }
        }
        if let Some(sess) = self.session {
            if s.t - sess.last_on_map > RETURN_WINDOW_SECS {
                fx.push(Effect::SessionEnded { key: sess.key, t: sess.last_on_map });
                self.session = None;
            }
        }

        if gain > 0 {
            self.last_gain_t = Some(s.t);
            if let Some(map) = s.map {
                let key = self.session_for(map, s.t, &mut fx);
                self.acc.entry(key).or_default().0 += gain;
            }
        }

        let dt = self.last_t.map_or(0, |prev| (s.t - prev).clamp(0, MAX_SAMPLE_GAP_SECS));
        self.last_t = Some(s.t);
        if let (Some(sess), Some(map)) = (self.session, s.map) {
            let recently_gained = self.last_gain_t.is_some_and(|g| s.t - g <= TRAINING_GRACE_SECS);
            if sess.map == map && recently_gained && dt > 0 {
                // dt ≤ MAX_SAMPLE_GAP_SECS, so the cast cannot truncate.
                self.acc.entry(sess.key).or_default().1 += dt as u32;
            }
        }
        fx
    }

    /// Flush the current minute and end the open session (app shutdown).
    pub fn finish(&mut self) -> Vec<Effect> {
        let mut fx = Vec::new();
        if let Some(minute) = self.minute.take() {
            self.flush(minute, &mut fx);
        }
        if let Some(sess) = self.session.take() {
            fx.push(Effect::SessionEnded { key: sess.key, t: sess.last_on_map });
        }
        fx
    }

    fn session_for(&mut self, map: MapId, t: i64, fx: &mut Vec<Effect>) -> SessionKey {
        if let Some(sess) = self.session {
            if sess.map == map {
                return sess.key;
            }
            fx.push(Effect::SessionEnded { key: sess.key, t: sess.last_on_map });
        }
        let key = SessionKey(self.next_key);
        self.next_key += 1;
        self.session = Some(ActiveSession { key, map, last_on_map: t });
        fx.push(Effect::SessionStarted { key, map, t });
        key
    }

    /// Returns the EXP gained since the baseline; 0 for first, rejected, or death readings.
    fn accept(&mut self, r: ExpReading) -> u64 {
        let Some(base) = self.baseline else {
            self.reset_to(r);
            return 0;
        };
        if base.pct_bp >= LEVEL_UP_FROM_BP && r.pct_bp < LEVEL_UP_TO_BP && r.exp < base.exp {
            let total = self
                .bounds
                .or_else(|| TotalBounds::from_reading(base))
                .map_or(base.exp, TotalBounds::estimate);
            self.reset_to(r);
            return total.saturating_sub(base.exp) + r.exp;
        }
        match (self.bounds, TotalBounds::from_reading(r)) {
            (Some(known), Some(implied)) => match known.intersect(implied) {
                Some(merged) => self.bounds = Some(merged),
                None => {
                    self.rejections += 1;
                    if self.rejections >= MAX_CONSECUTIVE_REJECTIONS {
                        self.reset_to(r);
                    }
                    return 0;
                }
            },
            (None, implied) => self.bounds = implied,
            (Some(_), None) => {}
        }
        self.rejections = 0;
        self.baseline = Some(r);
        r.exp.saturating_sub(base.exp)
    }

    fn reset_to(&mut self, r: ExpReading) {
        self.baseline = Some(r);
        self.bounds = TotalBounds::from_reading(r);
        self.rejections = 0;
    }

    fn roll_minute(&mut self, t: i64, fx: &mut Vec<Effect>) {
        let minute = t.div_euclid(60) * 60;
        match self.minute {
            Some(current) if current == minute => {}
            Some(current) => {
                self.flush(current, fx);
                self.minute = Some(minute);
            }
            None => self.minute = Some(minute),
        }
    }

    fn flush(&mut self, minute_start: i64, fx: &mut Vec<Effect>) {
        for (key, (exp_gained, training_secs)) in std::mem::take(&mut self.acc) {
            if exp_gained > 0 || training_secs > 0 {
                fx.push(Effect::Minute { minute_start, key, exp_gained, training_secs });
            }
        }
    }
}
```

- [ ] **Step 8: Run tracker tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml tracker::`
Expected: all pass (3 level, 3 confirm, 11 tracker). If one fails, re-derive the expected value by hand from the rules in the spec before changing code. The tests encode the spec.

- [ ] **Step 9: Commit**

```bash
git add src-tauri/src/tracker
git commit -m "feat: pure tracker for EXP gains, level-ups, sessions and minute rollup

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: Rates

**Files:**
- Create: `src-tauri/src/rates.rs`
- Modify: `src-tauri/src/lib.rs` (`pub mod rates;`)

**Interfaces:**
- Produces: `rates::MinuteRow { minute_start: i64, exp_gained: u64, training_secs: u32 }`, `rates::Bar { start, exp_gained, rate_per_hour: Option<f64> }` (Serialize camelCase), `BUCKET_SECS = 600`, `SMA_BUCKETS = 6`, `bucket_start(i64) -> i64`, `live_range(now, window_minutes: u32) -> (i64, i64)`, `rows_from(first_bar) -> i64`, `bars_range(&[MinuteRow], first_bar, last_bar) -> Vec<Bar>`, `best_hour_rate(&[MinuteRow]) -> Option<f64>`

- [ ] **Step 1: Write failing tests**

Create `src-tauri/src/rates.rs` with only:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn row(minute_start: i64, exp_gained: u64, training_secs: u32) -> MinuteRow {
        MinuteRow { minute_start, exp_gained, training_secs }
    }

    #[test]
    fn live_range_aligns_to_buckets() {
        assert_eq!(live_range(7_200, 120), (600, 7_200));
        assert_eq!(live_range(7_259, 180), (-3_000, 7_200));
    }

    #[test]
    fn bars_and_sma_match_hand_computation() {
        let rows = [row(600, 600, 60), row(1_200, 1_200, 60)];
        let (first, last) = live_range(7_200, 120);
        let bars = bars_range(&rows, first, last);
        assert_eq!(bars.len(), 12);
        assert_eq!(bars[0], Bar { start: 600, exp_gained: 600, rate_per_hour: Some(36_000.0) });
        assert_eq!(bars[1], Bar { start: 1_200, exp_gained: 1_200, rate_per_hour: Some(54_000.0) });
        assert_eq!(bars[6], Bar { start: 4_200, exp_gained: 0, rate_per_hour: Some(72_000.0) });
        assert_eq!(bars[7], Bar { start: 4_800, exp_gained: 0, rate_per_hour: None });
    }

    #[test]
    fn zero_training_buckets_do_not_dilute_the_rate() {
        // 30 min of training then a 10-min repot bucket with no training time.
        let rows: Vec<_> = (0..30).map(|m| row(m * 60, 100, 60)).collect();
        let bars = bars_range(&rows, 1_800, 1_800);
        assert_eq!(bars[0].rate_per_hour, Some(6_000.0));
    }

    #[test]
    fn empty_range_is_empty() {
        assert!(bars_range(&[], 1_200, 600).is_empty());
    }

    #[test]
    fn best_hour_ignores_short_spikes() {
        let mut rows: Vec<_> = (0..60).map(|m| row(m * 60, 100, 60)).collect();
        rows.push(row(120 * 60, 10_000, 60));
        assert_eq!(best_hour_rate(&rows), Some(6_000.0));
        assert_eq!(best_hour_rate(&rows[..10]), None);
    }
}
```
Add `pub mod rates;` to `src-tauri/src/lib.rs`.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --manifest-path src-tauri/Cargo.toml rates::`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

Prepend to `src-tauri/src/rates.rs`:
```rust
//! 10-minute bars and the 60-minute trailing EXP/hr line. Pure functions over minute rows.

use serde::Serialize;

pub const BUCKET_SECS: i64 = 600;
pub const SMA_BUCKETS: usize = 6;
/// A "best hour" must contain at least this much training time.
pub const MIN_BEST_WINDOW_SECS: u64 = 1_800;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MinuteRow {
    pub minute_start: i64,
    pub exp_gained: u64,
    pub training_secs: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bar {
    pub start: i64,
    pub exp_gained: u64,
    /// Σexp / Σtraining_secs × 3600 over this and the previous 5 buckets.
    pub rate_per_hour: Option<f64>,
}

pub fn bucket_start(t: i64) -> i64 {
    t.div_euclid(BUCKET_SECS) * BUCKET_SECS
}

/// First and last bar starts for a live window of `window_minutes` ending at `now`.
pub fn live_range(now: i64, window_minutes: u32) -> (i64, i64) {
    let last = bucket_start(now);
    let bars = (i64::from(window_minutes) * 60 / BUCKET_SECS).max(1);
    (last - (bars - 1) * BUCKET_SECS, last)
}

/// Earliest minute needed to compute the SMA of a bar starting at `first_bar`.
pub fn rows_from(first_bar: i64) -> i64 {
    first_bar - (SMA_BUCKETS as i64 - 1) * BUCKET_SECS
}

/// One bar per bucket from `first_bar` to `last_bar` inclusive (both bucket-aligned).
pub fn bars_range(rows: &[MinuteRow], first_bar: i64, last_bar: i64) -> Vec<Bar> {
    if last_bar < first_bar {
        return Vec::new();
    }
    let origin = rows_from(first_bar);
    // Non-negative: last_bar ≥ first_bar > origin.
    let count = ((last_bar - origin) / BUCKET_SECS + 1) as usize;
    let mut buckets = vec![(0u64, 0u64); count];
    for r in rows {
        if r.minute_start < origin {
            continue;
        }
        let i = ((r.minute_start - origin) / BUCKET_SECS) as usize;
        if let Some(b) = buckets.get_mut(i) {
            b.0 += r.exp_gained;
            b.1 += u64::from(r.training_secs);
        }
    }
    (SMA_BUCKETS - 1..count)
        .map(|i| {
            let window = &buckets[i + 1 - SMA_BUCKETS..=i];
            let exp: u64 = window.iter().map(|b| b.0).sum();
            let secs: u64 = window.iter().map(|b| b.1).sum();
            Bar {
                start: origin + i as i64 * BUCKET_SECS,
                exp_gained: buckets[i].0,
                rate_per_hour: (secs > 0).then(|| exp as f64 * 3600.0 / secs as f64),
            }
        })
        .collect()
}

/// Highest EXP/hr over any 60-minute window with ≥ MIN_BEST_WINDOW_SECS of training.
/// `rows` must be sorted by `minute_start`.
pub fn best_hour_rate(rows: &[MinuteRow]) -> Option<f64> {
    let mut best: Option<f64> = None;
    let (mut lo, mut exp, mut secs) = (0usize, 0u64, 0u64);
    for hi in 0..rows.len() {
        exp += rows[hi].exp_gained;
        secs += u64::from(rows[hi].training_secs);
        while rows[hi].minute_start - rows[lo].minute_start >= 3_600 {
            exp -= rows[lo].exp_gained;
            secs -= u64::from(rows[lo].training_secs);
            lo += 1;
        }
        if secs >= MIN_BEST_WINDOW_SECS {
            let rate = exp as f64 * 3600.0 / secs as f64;
            best = Some(best.map_or(rate, |b| b.max(rate)));
        }
    }
    best
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml rates::`
Expected: 5 passed.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src
git commit -m "feat: 10-minute bars, 60-minute SMA and best-hour rate

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: Storage

**Files:**
- Create: `src-tauri/src/store.rs`
- Modify: `src-tauri/src/lib.rs` (`pub mod store;`)

**Interfaces:**
- Consumes: `geom::{Calibration, Rect}`, `rates::MinuteRow`, `read::map_strip::Signature`, `tracker::MapId`
- Produces: `store::Store` with `open(&Path)`, `open_in_memory()`, `maps() -> Vec<MapRecord>`, `map_name(MapId) -> Option<String>`, `insert_map(&str, &Signature, t) -> MapId`, `rename_map(MapId, &str) -> bool`, `start_session(MapId, t) -> i64`, `end_session(i64, t)`, `add_minute(minute_start, session_id, exp: u64, secs: u32)`, `close_open_sessions() -> usize`, `minutes_between(from, to_exclusive) -> Vec<MinuteRow>`, `session_minutes(i64) -> Vec<MinuteRow>`, `map_minutes(MapId) -> Vec<MinuteRow>`, `map_totals() -> Vec<MapTotals>`, `sessions(Option<MapId>) -> Vec<SessionRecord>`, `calibration(&str) -> Option<Calibration>`, `save_calibration(&str, &Calibration)`. All return `Result<_, StoreError>`.
  - `MapRecord { id, name, signature }`, `MapTotals { map_id, name, sessions: u32, exp_gained: u64, training_secs: u64 }`, `SessionRecord { id, map_id, map_name, started_at, ended_at: Option<i64>, exp_gained: u64, training_secs: u64 }` (Serialize camelCase)

- [ ] **Step 1: Write failing tests**

Create `src-tauri/src/store.rs` with only:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn sig() -> Signature {
        Signature(vec![1, 2, 3])
    }

    #[test]
    fn schema_is_created_once_and_reopens() {
        let dir = std::env::temp_dir().join(format!("exp-tracker-store-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("t.db");
        {
            let s = Store::open(&path).expect("open new");
            s.insert_map("A", &sig(), 0).expect("insert");
        }
        let s = Store::open(&path).expect("reopen");
        assert_eq!(s.maps().expect("maps").len(), 1);
        drop(s); // Windows can't delete a file with an open handle.
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn maps_round_trip_and_rename() {
        let s = Store::open_in_memory().expect("open");
        let id = s.insert_map("Victoria Road Perion", &sig(), 100).expect("insert");
        let maps = s.maps().expect("maps");
        assert_eq!((maps[0].id, maps[0].name.as_str(), &maps[0].signature), (id, "Victoria Road Perion", &sig()));
        assert!(s.rename_map(id, "Perion").expect("rename"));
        assert!(!s.rename_map(id + 99, "Nowhere").expect("rename missing"));
        assert_eq!(s.map_name(id).expect("name"), Some("Perion".to_string()));
    }

    #[test]
    fn minutes_accumulate_and_aggregate() {
        let s = Store::open_in_memory().expect("open");
        let map = s.insert_map("A", &sig(), 0).expect("insert");
        let s1 = s.start_session(map, 0).expect("start");
        let s2 = s.start_session(map, 120).expect("start");
        s.add_minute(0, s1, 100, 30).expect("add");
        s.add_minute(0, s1, 50, 30).expect("add again");
        s.add_minute(60, s1, 10, 60).expect("add");
        s.add_minute(60, s2, 5, 60).expect("add");
        assert_eq!(
            s.minutes_between(0, 120).expect("between"),
            vec![
                MinuteRow { minute_start: 0, exp_gained: 150, training_secs: 60 },
                MinuteRow { minute_start: 60, exp_gained: 15, training_secs: 120 },
            ]
        );
        assert_eq!(s.session_minutes(s2).expect("session").len(), 1);
        assert_eq!(s.map_minutes(map).expect("map").len(), 2);
        let totals = s.map_totals().expect("totals");
        assert_eq!((totals[0].sessions, totals[0].exp_gained, totals[0].training_secs), (2, 165, 180));
    }

    #[test]
    fn sessions_list_and_close_open_ones() {
        let s = Store::open_in_memory().expect("open");
        let map = s.insert_map("A", &sig(), 0).expect("insert");
        let other = s.insert_map("B", &Signature(vec![9]), 0).expect("insert");
        let s1 = s.start_session(map, 0).expect("start");
        s.add_minute(0, s1, 100, 60).expect("add");
        s.add_minute(60, s1, 100, 60).expect("add");
        let s2 = s.start_session(other, 500).expect("start");
        s.end_session(s2, 560).expect("end");
        assert_eq!(s.close_open_sessions().expect("close"), 1);
        let all = s.sessions(None).expect("sessions");
        assert_eq!(all.len(), 2);
        let first = all.iter().find(|r| r.id == s1).expect("s1 listed");
        assert_eq!((first.ended_at, first.exp_gained, first.training_secs), (Some(120), 200, 120));
        assert_eq!(s.sessions(Some(other)).expect("filtered").len(), 1);
    }

    #[test]
    fn calibration_round_trip() {
        let s = Store::open_in_memory().expect("open");
        assert_eq!(s.calibration("1920x1080").expect("get"), None);
        let c = Calibration { exp: Rect { x: 1051, y: 1035, w: 140, h: 19 }, map: Rect { x: 62, y: 36, w: 240, h: 44 } };
        s.save_calibration("1920x1080", &c).expect("save");
        s.save_calibration("1920x1080", &c).expect("overwrite");
        assert_eq!(s.calibration("1920x1080").expect("get"), Some(c));
    }

    #[test]
    fn exp_beyond_i64_is_rejected() {
        let s = Store::open_in_memory().expect("open");
        let map = s.insert_map("A", &sig(), 0).expect("insert");
        let sess = s.start_session(map, 0).expect("start");
        assert!(matches!(s.add_minute(0, sess, u64::MAX, 1), Err(StoreError::Range(_))));
    }
}
```
Add `pub mod store;` to `src-tauri/src/lib.rs`.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --manifest-path src-tauri/Cargo.toml store::`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

Prepend to `src-tauri/src/store.rs`:
```rust
//! Single-file SQLite storage, compiled into the exe. Parsed numbers only — never images.
//! Default rollback journal (not WAL) so the data stays one file.

use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;

use crate::geom::{Calibration, Rect};
use crate::rates::MinuteRow;
use crate::read::map_strip::Signature;
use crate::tracker::MapId;

const SCHEMA_VERSION: i64 = 1;
const SCHEMA: &str = "
BEGIN;
CREATE TABLE maps (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    signature BLOB NOT NULL,
    first_seen INTEGER NOT NULL
);
CREATE TABLE sessions (
    id INTEGER PRIMARY KEY,
    map_id INTEGER NOT NULL REFERENCES maps(id),
    started_at INTEGER NOT NULL,
    ended_at INTEGER
);
CREATE INDEX sessions_map ON sessions(map_id);
CREATE TABLE minutes (
    minute_start INTEGER NOT NULL,
    session_id INTEGER NOT NULL REFERENCES sessions(id),
    exp_gained INTEGER NOT NULL,
    training_secs INTEGER NOT NULL,
    PRIMARY KEY (minute_start, session_id)
);
CREATE INDEX minutes_session ON minutes(session_id);
CREATE TABLE calibration (
    resolution TEXT PRIMARY KEY,
    exp_x INTEGER NOT NULL, exp_y INTEGER NOT NULL, exp_w INTEGER NOT NULL, exp_h INTEGER NOT NULL,
    map_x INTEGER NOT NULL, map_y INTEGER NOT NULL, map_w INTEGER NOT NULL, map_h INTEGER NOT NULL
);
PRAGMA user_version = 1;
COMMIT;
";

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("{0} is out of range for storage")]
    Range(&'static str),
    #[error("database was written by a newer version of the app (schema {0})")]
    NewerSchema(i64),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapRecord {
    pub id: MapId,
    pub name: String,
    pub signature: Signature,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapTotals {
    pub map_id: MapId,
    pub name: String,
    pub sessions: u32,
    pub exp_gained: u64,
    pub training_secs: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRecord {
    pub id: i64,
    pub map_id: MapId,
    pub map_name: String,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub exp_gained: u64,
    pub training_secs: u64,
}

pub struct Store {
    conn: Connection,
}

fn to_i64(value: u64, what: &'static str) -> Result<i64, StoreError> {
    i64::try_from(value).map_err(|_| StoreError::Range(what))
}

fn col_u64(row: &Row, idx: usize) -> rusqlite::Result<u64> {
    let v: i64 = row.get(idx)?;
    u64::try_from(v).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(idx, v))
}

fn col_u32(row: &Row, idx: usize) -> rusqlite::Result<u32> {
    let v: i64 = row.get(idx)?;
    u32::try_from(v).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(idx, v))
}

fn minute_row(row: &Row) -> rusqlite::Result<MinuteRow> {
    Ok(MinuteRow { minute_start: row.get(0)?, exp_gained: col_u64(row, 1)?, training_secs: col_u32(row, 2)? })
}

fn rect_at(row: &Row, first: usize) -> rusqlite::Result<Rect> {
    Ok(Rect { x: col_u32(row, first)?, y: col_u32(row, first + 1)?, w: col_u32(row, first + 2)?, h: col_u32(row, first + 3)? })
}

impl Store {
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        Self::init(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<Self, StoreError> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self, StoreError> {
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        match version {
            0 => conn.execute_batch(SCHEMA)?,
            SCHEMA_VERSION => {}
            newer => return Err(StoreError::NewerSchema(newer)),
        }
        Ok(Self { conn })
    }

    pub fn maps(&self) -> Result<Vec<MapRecord>, StoreError> {
        let mut stmt = self.conn.prepare("SELECT id, name, signature FROM maps ORDER BY id")?;
        let rows = stmt
            .query_map([], |r| Ok(MapRecord { id: r.get(0)?, name: r.get(1)?, signature: Signature(r.get(2)?) }))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn map_name(&self, id: MapId) -> Result<Option<String>, StoreError> {
        Ok(self.conn.query_row("SELECT name FROM maps WHERE id = ?1", [id], |r| r.get(0)).optional()?)
    }

    pub fn insert_map(&self, name: &str, signature: &Signature, t: i64) -> Result<MapId, StoreError> {
        self.conn.execute(
            "INSERT INTO maps (name, signature, first_seen) VALUES (?1, ?2, ?3)",
            params![name, signature.0.as_slice(), t],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// `false` when no map has that id.
    pub fn rename_map(&self, id: MapId, name: &str) -> Result<bool, StoreError> {
        Ok(self.conn.execute("UPDATE maps SET name = ?2 WHERE id = ?1", params![id, name])? == 1)
    }

    pub fn start_session(&self, map_id: MapId, t: i64) -> Result<i64, StoreError> {
        self.conn.execute("INSERT INTO sessions (map_id, started_at) VALUES (?1, ?2)", params![map_id, t])?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn end_session(&self, id: i64, t: i64) -> Result<(), StoreError> {
        self.conn.execute("UPDATE sessions SET ended_at = ?2 WHERE id = ?1", params![id, t])?;
        Ok(())
    }

    pub fn add_minute(&self, minute_start: i64, session_id: i64, exp_gained: u64, training_secs: u32) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO minutes (minute_start, session_id, exp_gained, training_secs) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (minute_start, session_id) DO UPDATE SET
               exp_gained = exp_gained + excluded.exp_gained,
               training_secs = training_secs + excluded.training_secs",
            params![minute_start, session_id, to_i64(exp_gained, "exp_gained")?, i64::from(training_secs)],
        )?;
        Ok(())
    }

    /// Close sessions left open by a crash or kill, at their last recorded minute.
    pub fn close_open_sessions(&self) -> Result<usize, StoreError> {
        Ok(self.conn.execute(
            "UPDATE sessions SET ended_at = COALESCE(
                (SELECT MAX(minute_start) + 60 FROM minutes WHERE session_id = sessions.id), started_at)
             WHERE ended_at IS NULL",
            [],
        )?)
    }

    /// Minute totals across all sessions, `from` inclusive to `to` exclusive.
    pub fn minutes_between(&self, from: i64, to: i64) -> Result<Vec<MinuteRow>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT minute_start, SUM(exp_gained), SUM(training_secs) FROM minutes
             WHERE minute_start >= ?1 AND minute_start < ?2 GROUP BY minute_start ORDER BY minute_start",
        )?;
        let rows = stmt.query_map(params![from, to], minute_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn session_minutes(&self, session_id: i64) -> Result<Vec<MinuteRow>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT minute_start, exp_gained, training_secs FROM minutes WHERE session_id = ?1 ORDER BY minute_start",
        )?;
        let rows = stmt.query_map([session_id], minute_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn map_minutes(&self, map_id: MapId) -> Result<Vec<MinuteRow>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT mi.minute_start, SUM(mi.exp_gained), SUM(mi.training_secs)
             FROM minutes mi JOIN sessions s ON s.id = mi.session_id
             WHERE s.map_id = ?1 GROUP BY mi.minute_start ORDER BY mi.minute_start",
        )?;
        let rows = stmt.query_map([map_id], minute_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn map_totals(&self) -> Result<Vec<MapTotals>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT m.id, m.name, COUNT(DISTINCT s.id), COALESCE(SUM(mi.exp_gained), 0), COALESCE(SUM(mi.training_secs), 0)
             FROM maps m JOIN sessions s ON s.map_id = m.id LEFT JOIN minutes mi ON mi.session_id = s.id
             GROUP BY m.id ORDER BY 5 DESC",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(MapTotals {
                    map_id: r.get(0)?,
                    name: r.get(1)?,
                    sessions: col_u32(r, 2)?,
                    exp_gained: col_u64(r, 3)?,
                    training_secs: col_u64(r, 4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn sessions(&self, map_id: Option<MapId>) -> Result<Vec<SessionRecord>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.map_id, m.name, s.started_at, s.ended_at,
                    COALESCE(SUM(mi.exp_gained), 0), COALESCE(SUM(mi.training_secs), 0)
             FROM sessions s JOIN maps m ON m.id = s.map_id LEFT JOIN minutes mi ON mi.session_id = s.id
             WHERE ?1 IS NULL OR s.map_id = ?1
             GROUP BY s.id ORDER BY s.started_at DESC LIMIT 500",
        )?;
        let rows = stmt
            .query_map([map_id], |r| {
                Ok(SessionRecord {
                    id: r.get(0)?,
                    map_id: r.get(1)?,
                    map_name: r.get(2)?,
                    started_at: r.get(3)?,
                    ended_at: r.get(4)?,
                    exp_gained: col_u64(r, 5)?,
                    training_secs: col_u64(r, 6)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn calibration(&self, resolution: &str) -> Result<Option<Calibration>, StoreError> {
        Ok(self
            .conn
            .query_row(
                "SELECT exp_x, exp_y, exp_w, exp_h, map_x, map_y, map_w, map_h FROM calibration WHERE resolution = ?1",
                [resolution],
                |r| Ok(Calibration { exp: rect_at(r, 0)?, map: rect_at(r, 4)? }),
            )
            .optional()?)
    }

    pub fn save_calibration(&self, resolution: &str, c: &Calibration) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO calibration (resolution, exp_x, exp_y, exp_w, exp_h, map_x, map_y, map_w, map_h)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT (resolution) DO UPDATE SET
               exp_x = excluded.exp_x, exp_y = excluded.exp_y, exp_w = excluded.exp_w, exp_h = excluded.exp_h,
               map_x = excluded.map_x, map_y = excluded.map_y, map_w = excluded.map_w, map_h = excluded.map_h",
            params![resolution, c.exp.x, c.exp.y, c.exp.w, c.exp.h, c.map.x, c.map.y, c.map.w, c.map.h],
        )?;
        Ok(())
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml store::`
Expected: 6 passed. If `?1 IS NULL OR s.map_id = ?1` fails to bind a `[Option<i64>]`, use `params![map_id]`. Both bind `Option<i64>` as NULL or an integer.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src
git commit -m "feat: single-file SQLite store for maps, sessions, minutes, calibration

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 8: Screen capture and the glyph-collect dev tool

**Files:**
- Create: `src-tauri/src/capture.rs`, `src-tauri/src/bin/glyph_collect.rs`
- Modify: `src-tauri/src/lib.rs` (`pub mod capture;`)

**Interfaces:**
- Consumes: `geom::{Calibration, Crop, Rect}`, `read::exp::{segment, classify}`, `read::glyphs::TEMPLATES`
- Produces:
  - `capture::Regions { width: u32, height: u32, calibration: Calibration }`
  - `capture::Shared { regions: RwLock<Option<Regions>>, full_frame_request: Mutex<Option<SyncSender<Crop>>> }` (Default)
  - `capture::CaptureMsg::{Regions { t: i64, exp: Crop, map: Crop }, Uncalibrated { width: u32, height: u32 }, Closed}`
  - `capture::Control` (= `CaptureControl<Handler, CaptureError>`), `capture::start(SyncSender<CaptureMsg>, Arc<Shared>) -> Result<Control, CaptureError>`, `capture::CaptureError`
  - `crate::now_unix() -> i64` (in `lib.rs`)

- [ ] **Step 1: Add `now_unix` to lib.rs**

In `src-tauri/src/lib.rs`, below the module list:
```rust
/// Current UTC time in unix seconds.
pub fn now_unix() -> i64 {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock is set after 1970")
        .as_secs();
    i64::try_from(secs).expect("unix seconds fit in i64")
}
```
Add `pub mod capture;`.

- [ ] **Step 2: Write capture.rs**

Create `src-tauri/src/capture.rs`. Hardware capture can't be unit-tested; Step 4 checks it by hand with the dev tool.
```rust
//! Windows Graphics Capture of the PRIMARY MONITOR (never a window, never the game
//! process). Each accepted frame is cropped to the calibrated regions on the GPU,
//! copied to memory, and sent to the pipeline. Nothing is written to disk.

use std::sync::mpsc::{SyncSender, TrySendError};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use windows_capture::capture::{CaptureControl, Context, GraphicsCaptureApiHandler};
use windows_capture::frame::Frame;
use windows_capture::graphics_capture_api::InternalCaptureControl;
use windows_capture::monitor::Monitor;
use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings, MinimumUpdateIntervalSettings,
    SecondaryWindowSettings, Settings,
};

use crate::geom::{Calibration, Crop, Rect};
use crate::now_unix;

pub const SAMPLE_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Regions {
    pub width: u32,
    pub height: u32,
    pub calibration: Calibration,
}

/// State shared between the capture thread, the pipeline and commands.
#[derive(Default)]
pub struct Shared {
    /// Calibration for the current monitor resolution; `None` until known.
    pub regions: RwLock<Option<Regions>>,
    /// One-shot request for a full frame (calibration screen).
    pub full_frame_request: Mutex<Option<SyncSender<Crop>>>,
}

#[derive(Debug)]
pub enum CaptureMsg {
    Regions { t: i64, exp: Crop, map: Crop },
    Uncalibrated { width: u32, height: u32 },
    Closed,
}

#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("monitor: {0}")]
    Monitor(#[from] windows_capture::monitor::Error),
    #[error("frame: {0}")]
    Frame(#[from] windows_capture::frame::Error),
    #[error("frame buffer has unexpected size")]
    BufferSize,
    #[error("shared capture state lock poisoned")]
    Poisoned,
    #[error("could not start capture: {0}")]
    Start(String),
}

pub struct Flags {
    pub tx: SyncSender<CaptureMsg>,
    pub shared: Arc<Shared>,
}

pub struct Handler {
    tx: SyncSender<CaptureMsg>,
    shared: Arc<Shared>,
    last_sent: Option<Instant>,
}

pub type Control = CaptureControl<Handler, CaptureError>;

fn copy_region(frame: &mut Frame, r: Rect) -> Result<Crop, CaptureError> {
    let buffer = frame.buffer_crop(r.x, r.y, r.x + r.w, r.y + r.h)?;
    let mut scratch = Vec::new();
    let pixels = buffer.as_nopadding_buffer(&mut scratch).to_vec();
    Crop::new(r.w, r.h, pixels).ok_or(CaptureError::BufferSize)
}

impl GraphicsCaptureApiHandler for Handler {
    type Flags = Flags;
    type Error = CaptureError;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        Ok(Self { tx: ctx.flags.tx, shared: ctx.flags.shared, last_sent: None })
    }

    fn on_frame_arrived(&mut self, frame: &mut Frame, control: InternalCaptureControl) -> Result<(), Self::Error> {
        let (width, height) = (frame.width(), frame.height());

        let requester = self.shared.full_frame_request.lock().map_err(|_| CaptureError::Poisoned)?.take();
        if let Some(requester) = requester {
            let full = copy_region(frame, Rect { x: 0, y: 0, w: width, h: height })?;
            if requester.try_send(full).is_err() {
                // The requester timed out and dropped its receiver; nobody to deliver to.
            }
        }

        if self.last_sent.is_some_and(|t| t.elapsed() < SAMPLE_INTERVAL) {
            return Ok(());
        }
        self.last_sent = Some(Instant::now());

        let regions = *self.shared.regions.read().map_err(|_| CaptureError::Poisoned)?;
        let msg = match regions {
            Some(r)
                if r.width == width
                    && r.height == height
                    && r.calibration.exp.fits_in(width, height)
                    && r.calibration.map.fits_in(width, height) =>
            {
                CaptureMsg::Regions {
                    t: now_unix(),
                    exp: copy_region(frame, r.calibration.exp)?,
                    map: copy_region(frame, r.calibration.map)?,
                }
            }
            _ => CaptureMsg::Uncalibrated { width, height },
        };
        match self.tx.try_send(msg) {
            // Full: the pipeline is behind; dropping one sample is correct (the next arrives in 1 s).
            Ok(()) | Err(TrySendError::Full(_)) => Ok(()),
            Err(TrySendError::Disconnected(_)) => {
                control.stop();
                Ok(())
            }
        }
    }

    fn on_closed(&mut self) -> Result<(), Self::Error> {
        // Best effort: the pipeline also notices via `Control::is_finished`.
        if self.tx.try_send(CaptureMsg::Closed).is_err() {
            // Channel full or pipeline gone; is_finished() covers both.
        }
        Ok(())
    }
}

/// Start capturing the primary monitor on a dedicated thread.
pub fn start(tx: SyncSender<CaptureMsg>, shared: Arc<Shared>) -> Result<Control, CaptureError> {
    let settings = Settings::new(
        Monitor::primary()?,
        CursorCaptureSettings::WithoutCursor,
        DrawBorderSettings::WithoutBorder,
        SecondaryWindowSettings::Default,
        MinimumUpdateIntervalSettings::Default,
        DirtyRegionSettings::Default,
        ColorFormat::Rgba8,
        Flags { tx, shared },
    );
    Handler::start_free_threaded(settings).map_err(|e| CaptureError::Start(format!("{e:?}")))
}
```

- [ ] **Step 3: Write the dev tool**

Create `src-tauri/src/bin/glyph_collect.rs`:
```rust
//! DEV ONLY (feature `dev-tools`, never shipped): print EXP-font glyphs the reader
//! doesn't know yet, as ready-to-paste `Template` entries for `read/glyphs.rs`.
//!
//!   cargo run --manifest-path src-tauri/Cargo.toml --features dev-tools --bin glyph-collect -- png <path> <x> <y> <w> <h>
//!   cargo run --manifest-path src-tauri/Cargo.toml --features dev-tools --bin glyph-collect -- live <x> <y> <w> <h> <seconds>

use std::error::Error;
use std::sync::mpsc::sync_channel;
use std::sync::Arc;
use std::time::{Duration, Instant};

use maple_exp_tracker_lib::capture::{self, CaptureMsg, Regions, Shared};
use maple_exp_tracker_lib::geom::{Calibration, Crop, Rect};
use maple_exp_tracker_lib::read::exp::{classify, segment};
use maple_exp_tracker_lib::read::glyphs::TEMPLATES;
use maple_exp_tracker_lib::read::ReadError;

struct Unknown {
    rows: Vec<String>,
    count: usize,
    context: String,
}

fn collect(crop: &Crop, unknown: &mut Vec<Unknown>) -> Result<(), ReadError> {
    let glyphs = segment(crop)?;
    let context: String = glyphs.iter().map(|g| classify(g, TEMPLATES).unwrap_or('?')).collect();
    for g in glyphs.iter().filter(|g| classify(g, TEMPLATES).is_none()) {
        match unknown.iter_mut().find(|u| u.rows == g.rows) {
            Some(u) => u.count += 1,
            None => unknown.push(Unknown { rows: g.rows.clone(), count: 1, context: context.clone() }),
        }
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let num = |i: usize| -> Result<u32, Box<dyn Error>> { Ok(args.get(i).ok_or("missing argument")?.parse()?) };
    let mut unknown = Vec::new();
    match args.first().map(String::as_str) {
        Some("png") => {
            let path = args.get(1).ok_or("missing png path")?;
            let rect = Rect { x: num(2)?, y: num(3)?, w: num(4)?, h: num(5)? };
            let crop = Crop::from_image(&image::open(path)?.to_rgba8()).sub(rect).ok_or("rect outside image")?;
            collect(&crop, &mut unknown)?;
        }
        Some("live") => {
            let rect = Rect { x: num(1)?, y: num(2)?, w: num(3)?, h: num(4)? };
            let seconds = num(5)?;
            let monitor = windows_capture::monitor::Monitor::primary()?;
            let shared = Arc::new(Shared::default());
            *shared.regions.write().map_err(|_| "regions lock poisoned")? = Some(Regions {
                width: monitor.width()?,
                height: monitor.height()?,
                calibration: Calibration { exp: rect, map: rect },
            });
            let (tx, rx) = sync_channel(4);
            let control = capture::start(tx, shared)?;
            let deadline = Instant::now() + Duration::from_secs(seconds.into());
            let mut frames = 0;
            while Instant::now() < deadline {
                if let Ok(CaptureMsg::Regions { exp, .. }) = rx.recv_timeout(Duration::from_secs(1)) {
                    frames += 1;
                    if let Err(e) = collect(&exp, &mut unknown) {
                        eprintln!("frame {frames}: {e}");
                    }
                }
            }
            control.stop().map_err(|e| format!("{e:?}"))?;
            eprintln!("{frames} frames read");
        }
        _ => return Err("usage: glyph-collect png <path> <x> <y> <w> <h> | live <x> <y> <w> <h> <seconds>".into()),
    }
    for u in &unknown {
        println!("// seen {}x; line read as {:?}", u.count, u.context);
        println!("Template {{ ch: '?', rows: &[");
        for r in &u.rows {
            println!("    \"{r}\",");
        }
        println!("] }},");
    }
    println!("// {} unknown glyph shape(s)", unknown.len());
    Ok(())
}
```

- [ ] **Step 4: Verify on the fixture and live**

```bash
cargo run --manifest-path src-tauri/Cargo.toml --features dev-tools --bin glyph-collect -- png src-tauri/tests/fixtures/exp_326_0.10.png 0 0 140 19
```
Expected: `// 0 unknown glyph shape(s)`. Every fixture glyph is already a template.

Then check live capture runs (MapleStory doesn't need to be open; this only proves capture works):
```bash
cargo run --manifest-path src-tauri/Cargo.toml --features dev-tools --bin glyph-collect -- live 1051 1035 140 19 3
```
Expected: stderr shows `N frames read` with N ≥ 2 and no panic. (Glyph output depends on what's on screen.)

- [ ] **Step 5: Verify release builds exclude the tool**

Run: `cargo build --manifest-path src-tauri/Cargo.toml --release --bins 2>&1 | tail -3` then `ls src-tauri/target/release/*.exe`
Expected: `maple-exp-tracker.exe` only, no `glyph-collect.exe`.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src
git commit -m "feat: primary-monitor WGC capture and dev-only glyph collector

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 9: HUMAN STEP — collect the remaining EXP glyphs

The fixture only contains `0 1 2 3 6 . % [ ]`. Digits `4 5 7 8 9` (and `,` if the game uses thousands separators) can only come from real play. **Stop and ask the user to do this step.** It needs the game running and EXP being gained.

**Files:**
- Modify: `src-tauri/src/read/glyphs.rs`, `src-tauri/src/read/exp.rs` (tests)

- [ ] **Step 1: Ask the user to play and run the collector**

Message to the user:
> I need about 5 minutes of real EXP gain to learn the rest of the digit shapes. Please have MapleStory open (borderless, 1920×1080 like before) and start grinding. Tell me when you're killing mobs, and I'll run a 5-minute capture of just the EXP numbers.

When they confirm, run (adjust the rect if their resolution differs; it's the EXP numbers box):
```bash
cargo run --manifest-path src-tauri/Cargo.toml --features dev-tools --bin glyph-collect -- live 1051 1035 140 19 300 > glyphs-collected.txt
```

- [ ] **Step 2: Label each unknown glyph**

Open `glyphs-collected.txt`. Each entry shows the ASCII glyph and the line it was read from, e.g. `"32?[0.1?%]"`. Identify the character from its shape. If unsure, use the line context: the EXP number increases over time, and the bracket shows a percent. Replace `'?'` with the character and paste the entries into `TEMPLATES` in `src-tauri/src/read/glyphs.rs`. Keep every variant: several shapes per digit are expected. Delete `glyphs-collected.txt` (it must not be committed).

- [ ] **Step 3: Re-run until nothing is unknown**

Ask the user for another short capture (60 s) and repeat Steps 1–2 until the tool prints `// 0 unknown glyph shape(s)` over a minute of play containing every digit 0–9 in the EXP number. If two different characters' templates make `classify` ambiguous (it returns `None` for a glyph that is clearly a known char), report it instead of loosening `MAX_MISMATCH_RATIO`.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/read
git commit -m "feat: complete EXP glyph templates from live capture

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 10: Pipeline, commands, and app wiring

**Files:**
- Create: `src-tauri/src/pipeline.rs`, `src-tauri/src/error.rs`, `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: everything above.
- Produces (IPC contract the frontend relies on):
  - Event `tracker-tick` payload `Tick` = `{ status: ReaderStatus, mapName: string|null, exp: number|null, pctBp: number|null, totalEst: number|null }`
  - `ReaderStatus` = `{kind:"ok"} | {kind:"expUnreadable"} | {kind:"mapUnreadable"} | {kind:"notCalibrated"} | {kind:"captureFailed",detail} | {kind:"storageFailed",detail} | {kind:"notifyFailed",detail}`
  - Event `tracker-minute` payload `null`
  - Commands (JS arg names in camelCase): `get_live_snapshot() -> Tick`, `get_live_chart({windowMinutes: 120|180}) -> Bar[]`, `get_map_summaries() -> MapSummary[]`, `rename_map({mapId, name}) -> null`, `get_sessions({mapId: number|null}) -> SessionRecord[]`, `get_session_chart({sessionId}) -> Bar[]`, `get_calibration_frame() -> {width, height, pngDataUrl, current: Calibration|null}`, `preview_regions({exp, map}) -> {exp: Preview<ExpReading>, map: Preview<string>}`, `save_calibration({exp, map}) -> null`
  - `Preview<T>` = `{kind:"ok", value:T} | {kind:"failed", reason:string}`
  - `MapSummary` = `{mapId, name, sessions, trainingSecs, expGained, avgRatePerHour: number|null, bestHourRate: number|null}`
  - Errors reject with `AppError` = `{kind: "storage"|"capture"|"invalid"|"notFound", message}`

- [ ] **Step 1: Write error.rs**

```rust
//! Error type crossing the IPC boundary.

use serde::Serialize;

use crate::store::StoreError;

#[derive(Debug, thiserror::Error, Serialize)]
#[serde(tag = "kind", content = "message", rename_all = "camelCase")]
pub enum AppError {
    #[error("storage: {0}")]
    Storage(String),
    #[error("capture: {0}")]
    Capture(String),
    #[error("invalid input: {0}")]
    Invalid(String),
    #[error("not found: {0}")]
    NotFound(String),
}

impl From<StoreError> for AppError {
    fn from(e: StoreError) -> Self {
        AppError::Storage(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_as_tagged_object() {
        let json = serde_json::to_string(&AppError::NotFound("map 3".into())).expect("serializes");
        assert_eq!(json, r#"{"kind":"notFound","message":"map 3"}"#);
    }
}
```

- [ ] **Step 2: Write pipeline.rs**

```rust
//! Glue between capture, readers, tracker and storage. Runs on its own thread; the
//! tracker and readers stay pure, all I/O happens here.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::capture::{self, CaptureMsg, Control, Regions, Shared};
use crate::geom::{resolution_key, Crop};
use crate::read::exp::read_exp;
use crate::read::map_strip::{read_strip, Signature};
use crate::read::ocr::MapOcr;
use crate::store::{Store, StoreError};
use crate::tracker::{ConfirmOutcome, Effect, MapConfirmer, MapId, Sample, SessionKey, Tracker};

pub const TICK_EVENT: &str = "tracker-tick";
pub const MINUTE_EVENT: &str = "tracker-minute";
const MAX_BACKOFF: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", content = "detail", rename_all = "camelCase")]
pub enum ReaderStatus {
    Ok,
    ExpUnreadable,
    MapUnreadable,
    NotCalibrated,
    CaptureFailed(String),
    StorageFailed(String),
    NotifyFailed(String),
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tick {
    pub status: ReaderStatus,
    pub map_name: Option<String>,
    pub exp: Option<u64>,
    pub pct_bp: Option<u32>,
    pub total_est: Option<u64>,
}

impl Default for Tick {
    fn default() -> Self {
        Self { status: ReaderStatus::NotCalibrated, map_name: None, exp: None, pct_bp: None, total_est: None }
    }
}

pub struct AppState {
    pub store: Mutex<Store>,
    pub capture: Arc<Shared>,
    /// Last full frame grabbed for the calibration screen (memory only).
    pub calibration_frame: Mutex<Option<Crop>>,
    pub live: RwLock<Tick>,
    shutdown: AtomicBool,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl AppState {
    pub fn new(store: Store) -> Self {
        Self {
            store: Mutex::new(store),
            capture: Arc::new(Shared::default()),
            calibration_frame: Mutex::new(None),
            live: RwLock::new(Tick::default()),
            shutdown: AtomicBool::new(false),
            thread: Mutex::new(None),
        }
    }
}

pub fn spawn(app: AppHandle, state: Arc<AppState>) -> std::io::Result<()> {
    let thread_state = Arc::clone(&state);
    let handle = std::thread::Builder::new()
        .name("exp-pipeline".into())
        .spawn(move || Pipeline::new(app, thread_state).run())?;
    *state.thread.lock().expect("thread slot lock poisoned") = Some(handle);
    Ok(())
}

/// Stop the pipeline, flushing the current minute and closing the open session.
pub fn shutdown(state: &AppState) {
    state.shutdown.store(true, Ordering::Release);
    let handle = state.thread.lock().expect("thread slot lock poisoned").take();
    if let Some(handle) = handle {
        if handle.join().is_err() {
            eprintln!("exp-pipeline thread panicked; the last minute may not be saved");
        }
    }
}

struct Pipeline {
    app: AppHandle,
    state: Arc<AppState>,
    tx: SyncSender<CaptureMsg>,
    rx: Receiver<CaptureMsg>,
    control: Option<Control>,
    retry_at: Instant,
    backoff: Duration,
    ocr: Result<MapOcr, String>,
    confirmer: MapConfirmer,
    tracker: Tracker,
    current_map: Option<MapId>,
    keys: HashMap<SessionKey, i64>,
    pending: Vec<Effect>,
    tried_resolution: Option<(u32, u32)>,
    capture_error: Option<String>,
    storage_error: Option<String>,
    notify_error: Option<String>,
}

impl Pipeline {
    fn new(app: AppHandle, state: Arc<AppState>) -> Self {
        let (tx, rx) = sync_channel(4);
        Self {
            app,
            state,
            tx,
            rx,
            control: None,
            retry_at: Instant::now(),
            backoff: Duration::from_secs(1),
            ocr: MapOcr::new().map_err(|e| e.to_string()),
            confirmer: MapConfirmer::new(),
            tracker: Tracker::new(),
            current_map: None,
            keys: HashMap::new(),
            pending: Vec::new(),
            tried_resolution: None,
            capture_error: None,
            storage_error: None,
            notify_error: None,
        }
    }

    fn run(mut self) {
        while !self.state.shutdown.load(Ordering::Acquire) {
            self.ensure_capture();
            match self.rx.recv_timeout(Duration::from_millis(500)) {
                Ok(CaptureMsg::Regions { t, exp, map }) => self.on_regions(t, exp, map),
                Ok(CaptureMsg::Uncalibrated { width, height }) => self.on_uncalibrated(width, height),
                Ok(CaptureMsg::Closed) => {}
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => unreachable!("the pipeline owns a sender"),
            }
        }
        let fx = self.tracker.finish();
        self.apply(fx);
        if let Some(control) = self.control.take() {
            if let Err(e) = control.stop() {
                eprintln!("stopping capture at shutdown: {e:?}");
            }
        }
    }

    fn ensure_capture(&mut self) {
        if let Some(finished) = self.control.take_if(|c| c.is_finished()) {
            self.capture_error = Some(match finished.into_thread_handle().join() {
                Ok(Ok(())) => "capture ended".to_string(),
                Ok(Err(e)) => format!("{e:?}"),
                Err(_) => "capture thread panicked".to_string(),
            });
            self.schedule_retry();
            self.publish(ReaderStatus::NotCalibrated);
        }
        if self.control.is_some() || Instant::now() < self.retry_at {
            return;
        }
        match capture::start(self.tx.clone(), Arc::clone(&self.state.capture)) {
            Ok(control) => self.control = Some(control),
            Err(e) => {
                self.capture_error = Some(e.to_string());
                self.schedule_retry();
                self.publish(ReaderStatus::NotCalibrated);
            }
        }
    }

    fn schedule_retry(&mut self) {
        self.retry_at = Instant::now() + self.backoff;
        self.backoff = (self.backoff * 2).min(MAX_BACKOFF);
    }

    fn frames_flowing(&mut self) {
        self.capture_error = None;
        self.backoff = Duration::from_secs(1);
    }

    fn on_uncalibrated(&mut self, width: u32, height: u32) {
        self.frames_flowing();
        if self.tried_resolution != Some((width, height)) {
            self.tried_resolution = Some((width, height));
            let found = self.state.store.lock().expect("store lock poisoned").calibration(&resolution_key(width, height));
            match found {
                Ok(Some(calibration)) => {
                    *self.state.capture.regions.write().expect("regions lock poisoned") =
                        Some(Regions { width, height, calibration });
                }
                Ok(None) => {}
                Err(e) => self.storage_error = Some(e.to_string()),
            }
        }
        self.publish(ReaderStatus::NotCalibrated);
    }

    fn on_regions(&mut self, t: i64, exp_crop: Crop, map_crop: Crop) {
        self.frames_flowing();
        let exp = read_exp(&exp_crop).ok();
        let mut reader = if exp.is_some() { ReaderStatus::Ok } else { ReaderStatus::ExpUnreadable };
        match read_strip(&map_crop) {
            Ok(strip) => {
                if let ConfirmOutcome::Changed { signature, strips } = self.confirmer.feed(strip.signature, map_crop) {
                    self.resolve_map(t, &signature, &strips);
                }
            }
            Err(_) if reader == ReaderStatus::Ok => reader = ReaderStatus::MapUnreadable,
            Err(_) => {}
        }
        let fx = self.tracker.on_sample(Sample { t, exp, map: self.current_map });
        let minute_written = fx.iter().any(|e| matches!(e, Effect::Minute { .. }));
        self.apply(fx);
        self.publish(reader);
        if minute_written {
            if let Err(e) = self.app.emit(MINUTE_EVENT, ()) {
                self.notify_error = Some(e.to_string());
            }
        }
    }

    fn resolve_map(&mut self, t: i64, signature: &Signature, strips: &[Crop]) {
        let result = self.find_map(signature).and_then(|found| match found {
            Some(id) => Ok(id),
            None => {
                let name = self.label(strips);
                self.state.store.lock().expect("store lock poisoned").insert_map(&name, signature, t)
            }
        });
        match result {
            Ok(id) => self.current_map = Some(id),
            Err(e) => {
                self.storage_error = Some(e.to_string());
                // Re-confirm on upcoming frames so the lookup is retried.
                self.confirmer = MapConfirmer::new();
            }
        }
    }

    fn find_map(&self, signature: &Signature) -> Result<Option<MapId>, StoreError> {
        let maps = self.state.store.lock().expect("store lock poisoned").maps()?;
        Ok(maps.into_iter().find(|m| m.signature.matches(signature)).map(|m| m.id))
    }

    /// OCR label for a new map. Failures produce a visible, renameable name.
    fn label(&self, strips: &[Crop]) -> String {
        match &self.ocr {
            Ok(ocr) => match ocr.label(strips) {
                Ok(Some(name)) => name,
                Ok(None) => "Unnamed map".to_string(),
                Err(e) => format!("Unnamed map (OCR failed: {e})"),
            },
            Err(e) => format!("Unnamed map (OCR unavailable: {e})"),
        }
    }

    /// Apply effects in order; on a storage error keep the rest queued for the next call.
    fn apply(&mut self, fx: Vec<Effect>) {
        self.pending.extend(fx);
        if self.pending.is_empty() {
            return;
        }
        let store = self.state.store.lock().expect("store lock poisoned");
        let mut done = 0;
        for effect in &self.pending {
            let result = match *effect {
                Effect::SessionStarted { key, map, t } => store.start_session(map, t).map(|id| {
                    self.keys.insert(key, id);
                }),
                Effect::SessionEnded { key, t } => {
                    let id = *self.keys.get(&key).expect("SessionStarted is applied before its SessionEnded");
                    store.end_session(id, t)
                }
                Effect::Minute { minute_start, key, exp_gained, training_secs } => {
                    let id = *self.keys.get(&key).expect("SessionStarted is applied before its minutes");
                    store.add_minute(minute_start, id, exp_gained, training_secs)
                }
            };
            match result {
                Ok(()) => done += 1,
                Err(e) => {
                    self.storage_error = Some(e.to_string());
                    break;
                }
            }
        }
        drop(store);
        self.pending.drain(..done);
        if self.pending.is_empty() {
            self.storage_error = None;
        }
    }

    fn publish(&mut self, reader: ReaderStatus) {
        let map_name = match self.current_map {
            Some(id) => match self.state.store.lock().expect("store lock poisoned").map_name(id) {
                Ok(name) => name,
                Err(e) => {
                    self.storage_error = Some(e.to_string());
                    None
                }
            },
            None => None,
        };
        let status = if let Some(e) = &self.capture_error {
            ReaderStatus::CaptureFailed(e.clone())
        } else if let Some(e) = &self.storage_error {
            ReaderStatus::StorageFailed(e.clone())
        } else if let Some(e) = &self.notify_error {
            ReaderStatus::NotifyFailed(e.clone())
        } else {
            reader
        };
        let baseline = self.tracker.baseline();
        let tick = Tick {
            status,
            map_name,
            exp: baseline.map(|r| r.exp),
            pct_bp: baseline.map(|r| r.pct_bp),
            total_est: self.tracker.total_estimate(),
        };
        *self.state.live.write().expect("live lock poisoned") = tick.clone();
        match self.app.emit(TICK_EVENT, &tick) {
            Ok(()) => self.notify_error = None,
            Err(e) => self.notify_error = Some(e.to_string()),
        }
    }
}
```

- [ ] **Step 3: Write commands.rs**

```rust
//! Tauri commands. Blocking ones run off the main thread via `#[tauri::command(async)]`.

use std::io::Cursor;
use std::sync::mpsc::sync_channel;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use serde::Serialize;
use tauri::State;

use crate::capture::Regions;
use crate::error::AppError;
use crate::geom::{resolution_key, Calibration, Crop, Rect};
use crate::now_unix;
use crate::pipeline::{AppState, Tick};
use crate::rates::{self, Bar};
use crate::read::exp::{read_exp, ExpReading};
use crate::read::map_strip::read_strip;
use crate::read::ocr::MapOcr;
use crate::store::SessionRecord;
use crate::tracker::MapId;

type AppStateRef<'a> = State<'a, Arc<AppState>>;

const MAX_MAP_NAME_LEN: usize = 80;

#[tauri::command]
pub fn get_live_snapshot(state: AppStateRef<'_>) -> Tick {
    state.live.read().expect("live lock poisoned").clone()
}

#[tauri::command(async)]
pub fn get_live_chart(state: AppStateRef<'_>, window_minutes: u32) -> Result<Vec<Bar>, AppError> {
    if !matches!(window_minutes, 120 | 180) {
        return Err(AppError::Invalid(format!("window must be 120 or 180 minutes, got {window_minutes}")));
    }
    let (first, last) = rates::live_range(now_unix(), window_minutes);
    let rows = state
        .store
        .lock()
        .expect("store lock poisoned")
        .minutes_between(rates::rows_from(first), last + rates::BUCKET_SECS)?;
    Ok(rates::bars_range(&rows, first, last))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MapSummary {
    map_id: MapId,
    name: String,
    sessions: u32,
    training_secs: u64,
    exp_gained: u64,
    avg_rate_per_hour: Option<f64>,
    best_hour_rate: Option<f64>,
}

#[tauri::command(async)]
pub fn get_map_summaries(state: AppStateRef<'_>) -> Result<Vec<MapSummary>, AppError> {
    let store = state.store.lock().expect("store lock poisoned");
    store
        .map_totals()?
        .into_iter()
        .map(|t| {
            let rows = store.map_minutes(t.map_id)?;
            Ok(MapSummary {
                map_id: t.map_id,
                name: t.name,
                sessions: t.sessions,
                training_secs: t.training_secs,
                exp_gained: t.exp_gained,
                avg_rate_per_hour: (t.training_secs > 0).then(|| t.exp_gained as f64 * 3600.0 / t.training_secs as f64),
                best_hour_rate: rates::best_hour_rate(&rows),
            })
        })
        .collect()
}

#[tauri::command(async)]
pub fn rename_map(state: AppStateRef<'_>, map_id: MapId, name: String) -> Result<(), AppError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > MAX_MAP_NAME_LEN {
        return Err(AppError::Invalid(format!("map name must be 1–{MAX_MAP_NAME_LEN} characters")));
    }
    if state.store.lock().expect("store lock poisoned").rename_map(map_id, name)? {
        Ok(())
    } else {
        Err(AppError::NotFound(format!("map {map_id}")))
    }
}

#[tauri::command(async)]
pub fn get_sessions(state: AppStateRef<'_>, map_id: Option<MapId>) -> Result<Vec<SessionRecord>, AppError> {
    Ok(state.store.lock().expect("store lock poisoned").sessions(map_id)?)
}

#[tauri::command(async)]
pub fn get_session_chart(state: AppStateRef<'_>, session_id: i64) -> Result<Vec<Bar>, AppError> {
    let rows = state.store.lock().expect("store lock poisoned").session_minutes(session_id)?;
    let (Some(first), Some(last)) = (rows.first(), rows.last()) else {
        return Ok(Vec::new());
    };
    Ok(rates::bars_range(&rows, rates::bucket_start(first.minute_start), rates::bucket_start(last.minute_start)))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationFrame {
    width: u32,
    height: u32,
    png_data_url: String,
    current: Option<Calibration>,
}

#[tauri::command(async)]
pub fn get_calibration_frame(state: AppStateRef<'_>) -> Result<CalibrationFrame, AppError> {
    let (tx, rx) = sync_channel::<Crop>(1);
    *state.capture.full_frame_request.lock().expect("frame request lock poisoned") = Some(tx);
    let frame = rx
        .recv_timeout(Duration::from_secs(3))
        .map_err(|_| AppError::Capture("no frame from screen capture within 3 s".into()))?;
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(frame.to_image())
        .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|e| AppError::Capture(format!("encoding preview: {e}")))?;
    let current = state.store.lock().expect("store lock poisoned").calibration(&resolution_key(frame.width, frame.height))?;
    let result = CalibrationFrame {
        width: frame.width,
        height: frame.height,
        png_data_url: format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(&png)),
        current,
    };
    *state.calibration_frame.lock().expect("calibration frame lock poisoned") = Some(frame);
    Ok(result)
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Preview<T> {
    Ok { value: T },
    Failed { reason: String },
}

#[derive(Serialize)]
pub struct RegionPreview {
    exp: Preview<ExpReading>,
    map: Preview<String>,
}

fn grabbed_frame(state: &AppState) -> Result<Crop, AppError> {
    state
        .calibration_frame
        .lock()
        .expect("calibration frame lock poisoned")
        .clone()
        .ok_or_else(|| AppError::Invalid("grab a frame first".into()))
}

#[tauri::command(async)]
pub fn preview_regions(state: AppStateRef<'_>, exp: Rect, map: Rect) -> Result<RegionPreview, AppError> {
    let frame = grabbed_frame(&state)?;
    let outside = || "box is outside the frame".to_string();
    let exp = match frame.sub(exp).ok_or_else(outside).and_then(|c| read_exp(&c).map_err(|e| e.to_string())) {
        Ok(value) => Preview::Ok { value },
        Err(reason) => Preview::Failed { reason },
    };
    let map = match frame.sub(map).ok_or_else(outside).and_then(|c| {
        read_strip(&c).map_err(|e| e.to_string())?;
        MapOcr::new()
            .and_then(|ocr| ocr.label(std::slice::from_ref(&c)))
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "map strip found, but OCR read no text".to_string())
    }) {
        Ok(value) => Preview::Ok { value },
        Err(reason) => Preview::Failed { reason },
    };
    Ok(RegionPreview { exp, map })
}

#[tauri::command(async)]
pub fn save_calibration(state: AppStateRef<'_>, exp: Rect, map: Rect) -> Result<(), AppError> {
    let frame = grabbed_frame(&state)?;
    let (width, height) = (frame.width, frame.height);
    if !exp.fits_in(width, height) || !map.fits_in(width, height) {
        return Err(AppError::Invalid(format!("boxes must lie inside the {width}x{height} frame")));
    }
    let calibration = Calibration { exp, map };
    state.store.lock().expect("store lock poisoned").save_calibration(&resolution_key(width, height), &calibration)?;
    *state.capture.regions.write().expect("regions lock poisoned") = Some(Regions { width, height, calibration });
    Ok(())
}
```

- [ ] **Step 4: Wire lib.rs**

Replace `src-tauri/src/lib.rs` entirely:
```rust
pub mod capture;
mod commands;
mod error;
pub mod geom;
mod pipeline;
pub mod rates;
pub mod read;
pub mod store;
pub mod tracker;

use std::sync::Arc;

use tauri::Manager;

/// Current UTC time in unix seconds.
pub fn now_unix() -> i64 {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock is set after 1970")
        .as_secs();
    i64::try_from(secs).expect("unix seconds fit in i64")
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let store = store::Store::open(&dir.join("tracker.db"))?;
            store.close_open_sessions()?;
            let state = Arc::new(pipeline::AppState::new(store));
            app.manage(Arc::clone(&state));
            pipeline::spawn(app.handle().clone(), state)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_live_snapshot,
            commands::get_live_chart,
            commands::get_map_summaries,
            commands::rename_map,
            commands::get_sessions,
            commands::get_session_chart,
            commands::get_calibration_frame,
            commands::preview_regions,
            commands::save_calibration,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                pipeline::shutdown(&app.state::<Arc<pipeline::AppState>>());
            }
        });
}
```

- [ ] **Step 5: Build and run all Rust tests**

```bash
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
```
Expected: clippy clean, all tests pass. If the Tauri 2.12 API differs (for example `#[tauri::command(async)]`, `Emitter::emit`, or `app.path().app_data_dir()`), check `~/.cargo/registry/src/*/tauri-2.12.*/src` and adapt. Don't drop the async attribute: blocking calls must stay off the main thread.

- [ ] **Step 6: Smoke test the app**

Run: `pnpm tauri dev`
Expected: the window opens with the placeholder text, and `%APPDATA%\com.mapleexp.tracker\tracker.db` exists. Close the window. The process exits within ~1 s, and `ls "$APPDATA/com.mapleexp.tracker"` shows only `tracker.db` (no images, no `-wal`/`-shm` files after exit).

- [ ] **Step 7: Commit**

```bash
git add src-tauri
git commit -m "feat: pipeline thread, IPC commands and events, app wiring

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 11: Frontend IPC layer

**Files:**
- Create: `src/lib/result.ts`, `src/lib/ipc.ts`, `src/lib/format.ts`, `src/lib/live.ts`, `src/lib/result.test.ts`, `src/lib/ipc.test.ts`, `src/lib/format.test.ts`, `src/lib/live.test.ts`

**Interfaces:**
- Consumes: the IPC contract from Task 10.
- Produces: `Result`, `ok`, `err`; types `Tick`, `ReaderStatus`, `Bar`, `MapSummary`, `Session`, `Rect`, `Calibration`, `CalibrationFrame`, `RegionPreview`, `AppError`; functions `getLiveSnapshot`, `getLiveChart`, `getMapSummaries`, `renameMap`, `getSessions`, `getSessionChart`, `getCalibrationFrame`, `previewRegions`, `saveCalibration`, `onTick`, `onMinute`, `toAppError`; formatters `formatExp`, `formatPct`, `formatHours`, `formatClock`, `formatDateTime`, `statusText`; `liveRates(bars, tick) -> LiveRates`.

- [ ] **Step 1: Write the failing tests**

`src/lib/result.test.ts`:
```ts
import { describe, expect, it } from "vitest";
import { err, ok } from "./result";

describe("result", () => {
  it("builds success and failure values", () => {
    expect(ok(3)).toEqual({ success: true, data: 3 });
    expect(err("x")).toEqual({ success: false, error: "x" });
  });
});
```

`src/lib/ipc.test.ts`:
```ts
import { describe, expect, it } from "vitest";
import { BarSchema, RegionPreviewSchema, TickSchema, toAppError } from "./ipc";

describe("ipc schemas", () => {
  it("parses a tick as Rust serializes it", () => {
    const tick = { status: { kind: "ok" }, mapName: "Victoria Road Perion", exp: 326, pctBp: 10, totalEst: 326000 };
    expect(TickSchema.parse(tick)).toEqual(tick);
    const failed = { ...tick, status: { kind: "captureFailed", detail: "no monitor" } };
    expect(TickSchema.parse(failed).status).toEqual({ kind: "captureFailed", detail: "no monitor" });
  });

  it("rejects unknown statuses and bad numbers", () => {
    expect(TickSchema.safeParse({ status: { kind: "nope" }, mapName: null, exp: null, pctBp: null, totalEst: null }).success).toBe(false);
    expect(BarSchema.safeParse({ start: 0, expGained: -1, ratePerHour: null }).success).toBe(false);
  });

  it("parses previews", () => {
    const p = { exp: { kind: "ok", value: { exp: 326, pctBp: 10 } }, map: { kind: "failed", reason: "region contains no text" } };
    expect(RegionPreviewSchema.parse(p)).toEqual(p);
  });

  it("maps rejections to AppError", () => {
    expect(toAppError({ kind: "notFound", message: "map 3" })).toEqual({ kind: "notFound", message: "map 3" });
    expect(toAppError(new Error("boom"))).toEqual({ kind: "ipc", message: "Error: boom" });
  });
});
```

`src/lib/format.test.ts`:
```ts
import { describe, expect, it } from "vitest";
import { formatExp, formatHours, formatPct, statusText } from "./format";

describe("format", () => {
  it("formats values", () => {
    expect(formatPct(10)).toBe("0.10%");
    expect(formatHours(1.5)).toBe("1h 30m");
    expect(formatHours(0.25)).toBe("15m");
    expect(formatHours(-1)).toBe("—");
    expect(formatExp(1_234_567)).toBe("1.23M");
  });

  it("describes statuses", () => {
    expect(statusText({ kind: "ok" })).toBe("Reading OK");
    expect(statusText({ kind: "storageFailed", detail: "disk full" })).toContain("disk full");
  });
});
```

`src/lib/live.test.ts`:
```ts
import { describe, expect, it } from "vitest";
import { liveRates } from "./live";
import type { Tick } from "./ipc";

const tick: Tick = { status: { kind: "ok" }, mapName: "A", exp: 250_000, pctBp: 2500, totalEst: 1_000_000 };

describe("liveRates", () => {
  it("derives %/hr and time to level from the latest bar", () => {
    const r = liveRates([{ start: 0, expGained: 1, ratePerHour: 100_000 }], tick);
    expect(r).toEqual({ expPerHour: 100_000, pctPerHour: 10, hoursToLevel: 7.5 });
  });

  it("returns nulls without a rate", () => {
    expect(liveRates([], tick)).toEqual({ expPerHour: null, pctPerHour: null, hoursToLevel: null });
    expect(liveRates([{ start: 0, expGained: 0, ratePerHour: null }], null)).toEqual({ expPerHour: null, pctPerHour: null, hoursToLevel: null });
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `pnpm test`
Expected: FAIL — modules `./result`, `./ipc`, `./format`, `./live` not found.

- [ ] **Step 3: Implement**

`src/lib/result.ts`:
```ts
export type Result<T, E> = { success: true; data: T } | { success: false; error: E };

export const ok = <T>(data: T): Result<T, never> => ({ success: true, data });
export const err = <E>(error: E): Result<never, E> => ({ success: false, error });
```

`src/lib/ipc.ts`:
```ts
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { z } from "zod";
import { err, ok, type Result } from "./result";

const count = z.number().int().nonnegative();

export const AppErrorSchema = z.object({
  kind: z.enum(["storage", "capture", "invalid", "notFound"]),
  message: z.string(),
});
export type AppError = z.infer<typeof AppErrorSchema> | { kind: "ipc"; message: string };

export const ReaderStatusSchema = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("ok") }),
  z.object({ kind: z.literal("expUnreadable") }),
  z.object({ kind: z.literal("mapUnreadable") }),
  z.object({ kind: z.literal("notCalibrated") }),
  z.object({ kind: z.literal("captureFailed"), detail: z.string() }),
  z.object({ kind: z.literal("storageFailed"), detail: z.string() }),
  z.object({ kind: z.literal("notifyFailed"), detail: z.string() }),
]);
export type ReaderStatus = z.infer<typeof ReaderStatusSchema>;

export const TickSchema = z.object({
  status: ReaderStatusSchema,
  mapName: z.string().nullable(),
  exp: count.nullable(),
  pctBp: count.nullable(),
  totalEst: count.nullable(),
});
export type Tick = z.infer<typeof TickSchema>;

export const BarSchema = z.object({ start: z.number().int(), expGained: count, ratePerHour: z.number().nonnegative().nullable() });
export type Bar = z.infer<typeof BarSchema>;

export const MapSummarySchema = z.object({
  mapId: z.number().int(),
  name: z.string(),
  sessions: count,
  trainingSecs: count,
  expGained: count,
  avgRatePerHour: z.number().nullable(),
  bestHourRate: z.number().nullable(),
});
export type MapSummary = z.infer<typeof MapSummarySchema>;

export const SessionSchema = z.object({
  id: z.number().int(),
  mapId: z.number().int(),
  mapName: z.string(),
  startedAt: z.number().int(),
  endedAt: z.number().int().nullable(),
  expGained: count,
  trainingSecs: count,
});
export type Session = z.infer<typeof SessionSchema>;

export const RectSchema = z.object({ x: count, y: count, w: count, h: count });
export type Rect = z.infer<typeof RectSchema>;
export const CalibrationSchema = z.object({ exp: RectSchema, map: RectSchema });
export type Calibration = z.infer<typeof CalibrationSchema>;

export const CalibrationFrameSchema = z.object({
  width: z.number().int().positive(),
  height: z.number().int().positive(),
  pngDataUrl: z.string().startsWith("data:image/png;base64,"),
  current: CalibrationSchema.nullable(),
});
export type CalibrationFrame = z.infer<typeof CalibrationFrameSchema>;

const previewOf = <T extends z.ZodType>(value: T) =>
  z.discriminatedUnion("kind", [
    z.object({ kind: z.literal("ok"), value }),
    z.object({ kind: z.literal("failed"), reason: z.string() }),
  ]);
export const RegionPreviewSchema = z.object({
  exp: previewOf(z.object({ exp: count, pctBp: count })),
  map: previewOf(z.string()),
});
export type RegionPreview = z.infer<typeof RegionPreviewSchema>;

/** Tauri rejects command promises with the serialized AppError; anything else is an IPC failure. */
export function toAppError(e: unknown): AppError {
  const parsed = AppErrorSchema.safeParse(e);
  return parsed.success ? parsed.data : { kind: "ipc", message: String(e) };
}

async function call<S extends z.ZodType>(cmd: string, schema: S, args?: Record<string, unknown>): Promise<Result<z.infer<S>, AppError>> {
  let raw: unknown;
  try {
    raw = await invoke(cmd, args);
  } catch (e) {
    return err(toAppError(e));
  }
  const parsed = schema.safeParse(raw);
  return parsed.success ? ok(parsed.data) : err({ kind: "ipc", message: `unexpected ${cmd} payload: ${parsed.error.message}` });
}

export const getLiveSnapshot = () => call("get_live_snapshot", TickSchema);
export const getLiveChart = (windowMinutes: 120 | 180) => call("get_live_chart", z.array(BarSchema), { windowMinutes });
export const getMapSummaries = () => call("get_map_summaries", z.array(MapSummarySchema));
export const renameMap = (mapId: number, name: string) => call("rename_map", z.null(), { mapId, name });
export const getSessions = (mapId: number | null) => call("get_sessions", z.array(SessionSchema), { mapId });
export const getSessionChart = (sessionId: number) => call("get_session_chart", z.array(BarSchema), { sessionId });
export const getCalibrationFrame = () => call("get_calibration_frame", CalibrationFrameSchema);
export const previewRegions = (c: Calibration) => call("preview_regions", RegionPreviewSchema, c);
export const saveCalibration = (c: Calibration) => call("save_calibration", z.null(), c);

async function subscribe(event: string, handler: (payload: unknown) => void): Promise<Result<UnlistenFn, AppError>> {
  try {
    return ok(await listen<unknown>(event, (e) => handler(e.payload)));
  } catch (e) {
    return err(toAppError(e));
  }
}

export const onTick = (cb: (tick: Result<Tick, AppError>) => void) =>
  subscribe("tracker-tick", (payload) => {
    const parsed = TickSchema.safeParse(payload);
    cb(parsed.success ? ok(parsed.data) : err({ kind: "ipc", message: `unexpected tick: ${parsed.error.message}` }));
  });

export const onMinute = (cb: () => void) => subscribe("tracker-minute", () => cb());
```

`src/lib/format.ts`:
```ts
import type { ReaderStatus } from "./ipc";

const compact = new Intl.NumberFormat("en-US", { notation: "compact", maximumFractionDigits: 2 });

export const formatExp = (n: number): string => compact.format(n);
export const formatPct = (bp: number): string => `${(bp / 100).toFixed(2)}%`;

export function formatHours(hours: number): string {
  if (!Number.isFinite(hours) || hours < 0) return "—";
  const total = Math.round(hours * 60);
  const h = Math.floor(total / 60);
  const m = total % 60;
  return h > 0 ? `${h}h ${m}m` : `${m}m`;
}

export const formatClock = (unix: number): string =>
  new Date(unix * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });

export const formatDateTime = (unix: number): string =>
  new Date(unix * 1000).toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });

export function statusText(s: ReaderStatus): string {
  switch (s.kind) {
    case "ok":
      return "Reading OK";
    case "expUnreadable":
      return "EXP not readable — is the status bar visible?";
    case "mapUnreadable":
      return "Map name not readable — is the minimap open at the top-left?";
    case "notCalibrated":
      return "Not calibrated for this screen resolution";
    case "captureFailed":
      return `Screen capture failed: ${s.detail}`;
    case "storageFailed":
      return `Saving failed: ${s.detail}`;
    case "notifyFailed":
      return `UI update failed: ${s.detail}`;
  }
}
```

`src/lib/live.ts`:
```ts
import type { Bar, Tick } from "./ipc";

export interface LiveRates {
  expPerHour: number | null;
  /** Percent of the current level per hour (2.5 = 2.5%/hr). */
  pctPerHour: number | null;
  hoursToLevel: number | null;
}

export function liveRates(bars: Bar[], tick: Tick | null): LiveRates {
  const rate = bars[bars.length - 1]?.ratePerHour ?? null;
  if (rate === null || rate <= 0) return { expPerHour: null, pctPerHour: null, hoursToLevel: null };
  const total = tick?.totalEst ?? null;
  const exp = tick?.exp ?? null;
  return {
    expPerHour: rate,
    pctPerHour: total ? (rate / total) * 100 : null,
    hoursToLevel: total !== null && exp !== null ? Math.max(0, total - exp) / rate : null,
  };
}
```

- [ ] **Step 4: Run tests and typecheck**

Run: `pnpm test && pnpm exec tsc --noEmit`
Expected: all tests pass, no type errors. If Zod 4 typing of `previewOf` complains, check `node_modules/zod` for the generic schema type name (`z.ZodType`) and adjust. Don't fall back to `any`.

- [ ] **Step 5: Commit**

```bash
git add src/lib
git commit -m "feat: typed, validated IPC layer and formatting helpers

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 12: App shell, chart, and live view

**Files:**
- Create: `src/App.css`, `src/components/RateChart.tsx`, `src/views/LiveView.tsx`
- Modify: `src/App.tsx`

**Interfaces:**
- Consumes: `lib/ipc`, `lib/format`, `lib/live`
- Produces: `RateChart({ bars }: { bars: Bar[] })`, `LiveView({ onCalibrate }: { onCalibrate: () => void })`; `App` with tabs `live | history | calibrate` (the History and Calibrate tabs render placeholders until Tasks 13–14).

- [ ] **Step 1: Write the theme and layout CSS**

`src/App.css`:
```css
:root {
  --bg: #f6f7f9;
  --surface: #ffffff;
  --text: #1c2230;
  --muted: #5b6475;
  --border: #dfe3ea;
  --accent: #2f6fde;
  --bar: #9db8ea;
  --line: #e0742f;
  --warn: #b25b00;
  color-scheme: light;
}
@media (prefers-color-scheme: dark) {
  :root:not([data-theme="light"]) {
    --bg: #12151b;
    --surface: #1b2029;
    --text: #e6e9ef;
    --muted: #98a1b3;
    --border: #2c3340;
    --accent: #6b9cf0;
    --bar: #3d5a8f;
    --line: #f29a5c;
    --warn: #f0b05a;
    color-scheme: dark;
  }
}
* { box-sizing: border-box; }
body { margin: 0; background: var(--bg); color: var(--text); font: 14px/1.4 system-ui, sans-serif; }
.app { max-width: 1100px; margin: 0 auto; padding: 16px; }
.tabs { display: flex; gap: 4px; margin-bottom: 16px; }
.tabs button { background: none; border: 1px solid var(--border); color: var(--text); padding: 6px 14px; border-radius: 6px; cursor: pointer; }
.tabs button.active { background: var(--accent); border-color: var(--accent); color: #fff; }
.card { background: var(--surface); border: 1px solid var(--border); border-radius: 8px; padding: 16px; margin-bottom: 16px; }
.stats { display: grid; grid-template-columns: repeat(auto-fit, minmax(160px, 1fr)); gap: 12px; }
.stat .label { color: var(--muted); font-size: 12px; }
.stat .value { font-size: 22px; font-variant-numeric: tabular-nums; }
.status { color: var(--muted); }
.status.warn { color: var(--warn); }
.row { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
table { width: 100%; border-collapse: collapse; }
th, td { text-align: left; padding: 6px 8px; border-bottom: 1px solid var(--border); font-variant-numeric: tabular-nums; }
tr.selectable { cursor: pointer; }
tr.selected { background: color-mix(in srgb, var(--accent) 15%, transparent); }
button, input { font: inherit; }
button.primary { background: var(--accent); color: #fff; border: none; padding: 6px 12px; border-radius: 6px; cursor: pointer; }
.error { color: var(--warn); }
```

- [ ] **Step 2: Write the chart**

`src/components/RateChart.tsx`:
```tsx
import { Bar as BarMark, ComposedChart, Legend, Line, ResponsiveContainer, Tooltip, XAxis, YAxis } from "recharts";
import type { Bar } from "../lib/ipc";
import { formatClock, formatExp } from "../lib/format";

export function RateChart({ bars }: { bars: Bar[] }) {
  const data = bars.map((b) => ({ label: formatClock(b.start), exp: b.expGained, rate: b.ratePerHour }));
  const tick = (v: unknown) => (typeof v === "number" ? formatExp(v) : String(v));
  return (
    <ResponsiveContainer width="100%" height={300}>
      <ComposedChart data={data} margin={{ top: 8, right: 8, bottom: 0, left: 0 }}>
        <XAxis dataKey="label" stroke="var(--muted)" />
        <YAxis yAxisId="exp" stroke="var(--muted)" tickFormatter={tick} />
        <YAxis yAxisId="rate" orientation="right" stroke="var(--muted)" tickFormatter={tick} />
        <Tooltip formatter={tick} contentStyle={{ background: "var(--surface)", borderColor: "var(--border)" }} />
        <Legend />
        <BarMark yAxisId="exp" dataKey="exp" name="EXP per 10 min" fill="var(--bar)" />
        <Line yAxisId="rate" dataKey="rate" name="EXP/hr (60-min avg)" stroke="var(--line)" strokeWidth={2} dot={false} connectNulls={false} />
      </ComposedChart>
    </ResponsiveContainer>
  );
}
```
Before writing it, check `node_modules/recharts/types` (Recharts 3.10.1) to confirm `ComposedChart`, `Bar`, `Line`, `YAxis yAxisId`, `ResponsiveContainer` and the `Tooltip formatter` signature. Adapt if the types differ.

- [ ] **Step 3: Write the live view**

`src/views/LiveView.tsx`:
```tsx
import { useCallback, useEffect, useState } from "react";
import { RateChart } from "../components/RateChart";
import { formatExp, formatHours, formatPct, statusText } from "../lib/format";
import { getLiveChart, getLiveSnapshot, onMinute, onTick, type AppError, type Bar, type Tick } from "../lib/ipc";
import { liveRates } from "../lib/live";

type WindowMinutes = 120 | 180;

export function LiveView({ onCalibrate }: { onCalibrate: () => void }) {
  const [tick, setTick] = useState<Tick | null>(null);
  const [bars, setBars] = useState<Bar[]>([]);
  const [windowMinutes, setWindowMinutes] = useState<WindowMinutes>(120);
  const [error, setError] = useState<AppError | null>(null);

  const refreshChart = useCallback(async () => {
    const res = await getLiveChart(windowMinutes);
    if (res.success) {
      setBars(res.data);
    } else {
      setError(res.error);
    }
  }, [windowMinutes]);

  useEffect(() => {
    void refreshChart();
  }, [refreshChart]);

  useEffect(() => {
    let disposed = false;
    const unlisteners: Array<() => void> = [];
    const keep = (res: Awaited<ReturnType<typeof onMinute>>) => {
      if (!res.success) return setError(res.error);
      if (disposed) res.data();
      else unlisteners.push(res.data);
    };
    void getLiveSnapshot().then((res) => {
      if (disposed) return;
      if (res.success) setTick(res.data);
      else setError(res.error);
    });
    void onTick((res) => (res.success ? setTick(res.data) : setError(res.error))).then(keep);
    void onMinute(() => void refreshChart()).then(keep);
    return () => {
      disposed = true;
      unlisteners.forEach((u) => u());
    };
  }, [refreshChart]);

  const rates = liveRates(bars, tick);
  const status = tick?.status ?? null;

  return (
    <>
      <div className="card">
        <div className="row" style={{ justifyContent: "space-between" }}>
          <h2 style={{ margin: 0 }}>{tick?.mapName ?? "Waiting for map…"}</h2>
          <span className={status?.kind === "ok" ? "status" : "status warn"}>
            {status ? statusText(status) : "Starting…"}
            {status?.kind === "notCalibrated" && (
              <>
                {" "}
                <button className="primary" onClick={onCalibrate}>Calibrate</button>
              </>
            )}
          </span>
        </div>
        {error && <p className="error">{error.message}</p>}
        <div className="stats" style={{ marginTop: 12 }}>
          <Stat label="EXP/hr (last 60 min)" value={rates.expPerHour === null ? "—" : formatExp(rates.expPerHour)} />
          <Stat label="%/hr" value={rates.pctPerHour === null ? "—" : `${rates.pctPerHour.toFixed(2)}%`} />
          <Stat label="Time to level" value={rates.hoursToLevel === null ? "—" : formatHours(rates.hoursToLevel)} />
          <Stat label="Current EXP" value={tick?.exp == null || tick.pctBp == null ? "—" : `${formatExp(tick.exp)} (${formatPct(tick.pctBp)})`} />
        </div>
      </div>
      <div className="card">
        <div className="row" style={{ justifyContent: "flex-end", marginBottom: 8 }}>
          {([120, 180] as const).map((m) => (
            <button key={m} className={m === windowMinutes ? "primary" : ""} onClick={() => setWindowMinutes(m)}>
              {m / 60}h
            </button>
          ))}
        </div>
        <RateChart bars={bars} />
      </div>
    </>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="stat">
      <div className="label">{label}</div>
      <div className="value">{value}</div>
    </div>
  );
}
```

- [ ] **Step 4: Write the app shell**

`src/App.tsx`:
```tsx
import { useState } from "react";
import "./App.css";
import { LiveView } from "./views/LiveView";

type Tab = "live" | "history" | "calibrate";
const TABS: ReadonlyArray<[Tab, string]> = [
  ["live", "Live"],
  ["history", "History"],
  ["calibrate", "Calibrate"],
];

export default function App() {
  const [tab, setTab] = useState<Tab>("live");
  return (
    <div className="app">
      <nav className="tabs">
        {TABS.map(([id, label]) => (
          <button key={id} className={id === tab ? "active" : ""} onClick={() => setTab(id)}>
            {label}
          </button>
        ))}
      </nav>
      {tab === "live" && <LiveView onCalibrate={() => setTab("calibrate")} />}
      {tab === "history" && <p>History — Task 13</p>}
      {tab === "calibrate" && <p>Calibrate — Task 14</p>}
    </div>
  );
}
```

- [ ] **Step 5: Typecheck, test, and look at it**

Run: `pnpm exec tsc --noEmit && pnpm test`, then `pnpm tauri dev`.
Expected: the Live tab shows "Not calibrated for this screen resolution" with a Calibrate button, an empty chart with 12 bars' worth of time labels, and the 2h/3h toggle switching between 12 and 18 buckets.

- [ ] **Step 6: Commit**

```bash
git add src
git commit -m "feat: app shell, rate chart and live view

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 13: History view

**Files:**
- Create: `src/views/HistoryView.tsx`
- Modify: `src/App.tsx` (render `<HistoryView />` for the history tab)

**Interfaces:**
- Consumes: `getMapSummaries`, `renameMap`, `getSessions`, `getSessionChart`, `RateChart`, formatters.

- [ ] **Step 1: Write the view**

`src/views/HistoryView.tsx`:
```tsx
import { useCallback, useEffect, useState } from "react";
import { RateChart } from "../components/RateChart";
import { formatDateTime, formatExp, formatHours } from "../lib/format";
import { getMapSummaries, getSessionChart, getSessions, renameMap, type AppError, type Bar, type MapSummary, type Session } from "../lib/ipc";

const rate = (v: number | null) => (v === null ? "—" : formatExp(v));

export function HistoryView() {
  const [maps, setMaps] = useState<MapSummary[]>([]);
  const [mapId, setMapId] = useState<number | null>(null);
  const [sessions, setSessions] = useState<Session[]>([]);
  const [sessionId, setSessionId] = useState<number | null>(null);
  const [sessionBars, setSessionBars] = useState<Bar[]>([]);
  const [editing, setEditing] = useState<{ id: number; name: string } | null>(null);
  const [error, setError] = useState<AppError | null>(null);

  const loadMaps = useCallback(async () => {
    const res = await getMapSummaries();
    if (res.success) setMaps(res.data);
    else setError(res.error);
  }, []);

  useEffect(() => {
    void loadMaps();
  }, [loadMaps]);

  useEffect(() => {
    void getSessions(mapId).then((res) => (res.success ? setSessions(res.data) : setError(res.error)));
    setSessionId(null);
  }, [mapId]);

  useEffect(() => {
    if (sessionId === null) return setSessionBars([]);
    void getSessionChart(sessionId).then((res) => (res.success ? setSessionBars(res.data) : setError(res.error)));
  }, [sessionId]);

  const saveRename = async () => {
    if (!editing) return;
    const res = await renameMap(editing.id, editing.name);
    if (!res.success) return setError(res.error);
    setEditing(null);
    await loadMaps();
  };

  return (
    <>
      {error && <p className="error">{error.message}</p>}
      <div className="card">
        <h3>Maps</h3>
        <table>
          <thead>
            <tr><th>Map</th><th>Sessions</th><th>Training</th><th>Avg EXP/hr</th><th>Best hour</th><th /></tr>
          </thead>
          <tbody>
            {maps.map((m) => (
              <tr key={m.mapId} className={`selectable ${m.mapId === mapId ? "selected" : ""}`} onClick={() => setMapId(m.mapId === mapId ? null : m.mapId)}>
                <td>
                  {editing?.id === m.mapId ? (
                    <input value={editing.name} autoFocus onClick={(e) => e.stopPropagation()} onChange={(e) => setEditing({ id: m.mapId, name: e.target.value })} onKeyDown={(e) => e.key === "Enter" && void saveRename()} />
                  ) : (
                    m.name
                  )}
                </td>
                <td>{m.sessions}</td>
                <td>{formatHours(m.trainingSecs / 3600)}</td>
                <td>{rate(m.avgRatePerHour)}</td>
                <td>{rate(m.bestHourRate)}</td>
                <td>
                  {editing?.id === m.mapId ? (
                    <button className="primary" onClick={(e) => { e.stopPropagation(); void saveRename(); }}>Save</button>
                  ) : (
                    <button onClick={(e) => { e.stopPropagation(); setEditing({ id: m.mapId, name: m.name }); }}>Rename</button>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        {maps.length === 0 && <p className="status">No training sessions recorded yet.</p>}
      </div>
      <div className="card">
        <h3>{mapId === null ? "All sessions" : "Sessions on this map"}</h3>
        <table>
          <thead>
            <tr><th>Started</th><th>Map</th><th>Training</th><th>EXP</th><th>EXP/hr</th></tr>
          </thead>
          <tbody>
            {sessions.map((s) => (
              <tr key={s.id} className={`selectable ${s.id === sessionId ? "selected" : ""}`} onClick={() => setSessionId(s.id)}>
                <td>{formatDateTime(s.startedAt)}{s.endedAt === null ? " (open)" : ""}</td>
                <td>{s.mapName}</td>
                <td>{formatHours(s.trainingSecs / 3600)}</td>
                <td>{formatExp(s.expGained)}</td>
                <td>{s.trainingSecs > 0 ? formatExp((s.expGained * 3600) / s.trainingSecs) : "—"}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      {sessionId !== null && (
        <div className="card">
          <RateChart bars={sessionBars} />
        </div>
      )}
    </>
  );
}
```
In `src/App.tsx` import `HistoryView` and replace the history placeholder with `<HistoryView />`.

- [ ] **Step 2: Typecheck and test**

Run: `pnpm exec tsc --noEmit && pnpm test`
Expected: clean.

- [ ] **Step 3: Commit**

```bash
git add src
git commit -m "feat: history view with per-map summaries, rename, session charts

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 14: Calibrate view

**Files:**
- Create: `src/views/CalibrateView.tsx`
- Modify: `src/App.tsx` (render `<CalibrateView />` for the calibrate tab)

**Interfaces:**
- Consumes: `getCalibrationFrame`, `previewRegions`, `saveCalibration`, `Rect`, `RegionPreview`.

- [ ] **Step 1: Write the view**

`src/views/CalibrateView.tsx`:
```tsx
import { useRef, useState, type MouseEvent } from "react";
import { formatPct } from "../lib/format";
import { getCalibrationFrame, previewRegions, saveCalibration, type AppError, type CalibrationFrame, type Rect, type RegionPreview } from "../lib/ipc";

type Target = "exp" | "map";
const DEFAULT_MAP: Rect = { x: 62, y: 36, w: 240, h: 44 };
const COLORS: Record<Target, string> = { exp: "#2f6fde", map: "#e0742f" };

export function CalibrateView() {
  const [frame, setFrame] = useState<CalibrationFrame | null>(null);
  const [rects, setRects] = useState<Record<Target, Rect | null>>({ exp: null, map: DEFAULT_MAP });
  const [target, setTarget] = useState<Target>("exp");
  const [drag, setDrag] = useState<{ x: number; y: number } | null>(null);
  const [preview, setPreview] = useState<RegionPreview | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<AppError | null>(null);
  const imgRef = useRef<HTMLImageElement>(null);

  const grab = async () => {
    setError(null);
    setPreview(null);
    const res = await getCalibrationFrame();
    if (!res.success) return setError(res.error);
    setFrame(res.data);
    setRects({ exp: res.data.current?.exp ?? null, map: res.data.current?.map ?? DEFAULT_MAP });
  };

  const toImage = (e: MouseEvent) => {
    const img = imgRef.current;
    if (!img || !frame) return null;
    const b = img.getBoundingClientRect();
    const clamp = (v: number, max: number) => Math.min(Math.max(Math.round(v), 0), max);
    return {
      x: clamp(((e.clientX - b.left) * frame.width) / b.width, frame.width),
      y: clamp(((e.clientY - b.top) * frame.height) / b.height, frame.height),
    };
  };

  const onMove = (e: MouseEvent) => {
    const p = toImage(e);
    if (!drag || !p) return;
    setRects((r) => ({ ...r, [target]: { x: Math.min(drag.x, p.x), y: Math.min(drag.y, p.y), w: Math.abs(p.x - drag.x), h: Math.abs(p.y - drag.y) } }));
  };

  const setField = (key: keyof Rect, value: string) => {
    const n = Number.parseInt(value, 10);
    const current = rects[target];
    if (!current || Number.isNaN(n) || n < 0) return;
    setRects((r) => ({ ...r, [target]: { ...current, [key]: n } }));
  };

  const both = rects.exp && rects.map ? { exp: rects.exp, map: rects.map } : null;

  const runPreview = async () => {
    if (!both) return;
    const res = await previewRegions(both);
    if (res.success) setPreview(res.data);
    else setError(res.error);
  };

  const save = async () => {
    if (!both) return;
    const res = await saveCalibration(both);
    if (res.success) setMessage("Saved. Tracking uses these boxes from now on.");
    else setError(res.error);
  };

  const active = rects[target];
  return (
    <>
      <div className="card">
        <p className="status">
          Open MapleStory, then grab a frame. Draw the <b style={{ color: COLORS.exp }}>EXP box</b> tightly around the numbers after
          "EXP." (leave room on the right for longer numbers). Check that the <b style={{ color: COLORS.map }}>map box</b> covers the
          street and map name under the minimap title.
        </p>
        <div className="row">
          <button className="primary" onClick={() => void grab()}>Grab frame</button>
          {(["exp", "map"] as const).map((t) => (
            <button key={t} className={t === target ? "primary" : ""} onClick={() => setTarget(t)}>
              Edit {t === "exp" ? "EXP box" : "map box"}
            </button>
          ))}
          <button disabled={!both || !frame} onClick={() => void runPreview()}>Preview</button>
          <button className="primary" disabled={!both || !frame} onClick={() => void save()}>Save</button>
        </div>
        {active && (
          <div className="row" style={{ marginTop: 8 }}>
            {(["x", "y", "w", "h"] as const).map((k) => (
              <label key={k}>
                {k} <input type="number" min={0} value={active[k]} onChange={(e) => setField(k, e.target.value)} style={{ width: 80 }} />
              </label>
            ))}
          </div>
        )}
        {error && <p className="error">{error.message}</p>}
        {message && <p className="status">{message}</p>}
        {preview && (
          <p>
            EXP: {preview.exp.kind === "ok" ? `${preview.exp.value.exp} (${formatPct(preview.exp.value.pctBp)})` : `✗ ${preview.exp.reason}`}
            <br />
            Map: {preview.map.kind === "ok" ? preview.map.value : `✗ ${preview.map.reason}`}
          </p>
        )}
      </div>
      {frame && (
        <div
          className="card"
          style={{ position: "relative", padding: 0, overflow: "hidden", cursor: "crosshair", userSelect: "none" }}
          onMouseDown={(e) => setDrag(toImage(e))}
          onMouseMove={onMove}
          onMouseUp={() => setDrag(null)}
          onMouseLeave={() => setDrag(null)}
        >
          <img ref={imgRef} src={frame.pngDataUrl} alt="Captured frame" draggable={false} style={{ width: "100%", display: "block" }} />
          {(["exp", "map"] as const).map((t) => {
            const r = rects[t];
            return (
              r && (
                <div
                  key={t}
                  style={{
                    position: "absolute",
                    left: `${(r.x / frame.width) * 100}%`,
                    top: `${(r.y / frame.height) * 100}%`,
                    width: `${(r.w / frame.width) * 100}%`,
                    height: `${(r.h / frame.height) * 100}%`,
                    outline: `2px solid ${COLORS[t]}`,
                    pointerEvents: "none",
                  }}
                />
              )
            );
          })}
        </div>
      )}
    </>
  );
}
```
In `src/App.tsx` import `CalibrateView` and replace the calibrate placeholder with `<CalibrateView />`.

- [ ] **Step 2: Typecheck and test**

Run: `pnpm exec tsc --noEmit && pnpm test`
Expected: clean.

- [ ] **Step 3: Manual check with the game**

Ask the user to open MapleStory. Run `pnpm tauri dev`, then go to Calibrate → Grab frame → draw the EXP box around the numbers (or type `1051 1035 140 19` at 1920×1080) → Preview.
Expected: EXP shows the in-game number and percent. Map shows the minimap name. Save, then switch to Live: within ~5 s the status reads "Reading OK", the map name appears, and Current EXP matches the game.

- [ ] **Step 4: Commit**

```bash
git add src
git commit -m "feat: calibration view with drag boxes, numeric fine-tune and live preview

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 15: Release build and end-to-end verification

**Files:**
- Modify: none, unless verification finds a bug (fix it with a test first).

- [ ] **Step 1: Full checks**

```bash
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
pnpm exec tsc --noEmit
```
Expected: all clean and green.

- [ ] **Step 2: Build the installer**

Run: `pnpm tauri build`
Expected: `src-tauri/target/release/bundle/nsis/Maple EXP Tracker_*_x64-setup.exe` exists.

- [ ] **Step 3: ToS self-audit**

Run each and confirm the expected result:
```bash
grep -rnE "OpenProcess|ReadProcessMemory|WriteProcessMemory|SendInput|keybd_event|mouse_event|SetWindowsHookEx|CreateRemoteThread|FindWindow" src-tauri/src || echo "none"
grep -rnE "from_window|Window::|window_name|window_hwnd" src-tauri/src || echo "none"
grep -rnE "save_as_image|imwrite|\.save\(" src-tauri/src --include=*.rs | grep -v "src/bin/" || echo "none"
```
Expected: `none` three times (`Window::` must not appear in capture code; the only image writes allowed are in `src/bin/`).

- [ ] **Step 4: HUMAN STEP — one hour of real play**

Ask the user to install the NSIS build, calibrate, note the in-game EXP at the start, grind for ~60 minutes on one map (with at least one repot trip if natural), then note the in-game EXP at the end.
Verify:
- History shows one session on that map (the repot trip didn't split it, if it was under 10 minutes).
- Session EXP gained equals end − start (plus level total if they leveled). Any difference should be a few seconds of EXP from the start/end edges. A larger difference is a bug to investigate with `systematic-debugging`.
- `%APPDATA%\com.mapleexp.tracker\` contains only `tracker.db` after the app is closed.
- The Live chart shows 10-minute bars, and the rate line doesn't dip during the repot bucket.

- [ ] **Step 5: Commit any fixes and tag**

```bash
git tag v0.1.0
```
(Only after Step 4 passes. Contacting Nexon for written approval is the user's step before any public release. Remind them.)
