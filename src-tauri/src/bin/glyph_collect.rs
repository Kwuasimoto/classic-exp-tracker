//! DEV ONLY (feature `dev-tools`, never shipped): tools for testing the EXP reader.
//!
//! record: print the EXP box's pixels for each frame as text, to test the reader with.
//! replay: run the EXP reader over a recording and print what each frame read as.
//! live / png: print the lines the reader can't read, with the closest characters.
//!
//!   cargo run --manifest-path src-tauri/Cargo.toml --features dev-tools --bin glyph-collect -- record <x> <y> <w> <h> <seconds> > frames.txt
//!   cargo run --manifest-path src-tauri/Cargo.toml --features dev-tools --bin glyph-collect -- replay frames.txt
//!   cargo run --manifest-path src-tauri/Cargo.toml --features dev-tools --bin glyph-collect -- live <x> <y> <w> <h> <seconds>
//!   cargo run --manifest-path src-tauri/Cargo.toml --features dev-tools --bin glyph-collect -- png <path> <x> <y> <w> <h>

use std::collections::BTreeMap;
use std::error::Error;
use std::sync::mpsc::sync_channel;
use std::sync::Arc;
use std::time::{Duration, Instant};

use classic_exp_tracker_lib::capture::{self, CaptureMsg, Regions, Shared};
use classic_exp_tracker_lib::geom::{Calibration, Crop, Rect};
use classic_exp_tracker_lib::read::exp::{read_exp, read_line};

/// Unreadable lines, keyed by what they read as with the closest characters of each
/// glyph not recognised, with a count.
type Rejected = BTreeMap<String, usize>;

fn collect_rejected(crop: &Crop, rejected: &mut Rejected) {
    let Ok(line) = read_line(crop) else { return };
    for g in line.glyphs.iter().filter(|g| g.ch.is_none()) {
        let near: Vec<String> = g.ranked.iter().take(3).map(|(ch, score)| format!("{ch:?} {score:.3}")).collect();
        *rejected.entry(format!("{:?} column {}: {}", line.text(), g.x, near.join(", "))).or_default() += 1;
    }
}

/// One frame of the EXP box as text: `frame <t>`, then one line of `rrggbb` per row.
fn print_pixels(t: i64, crop: &Crop) {
    println!("frame {t}");
    for y in 0..crop.height {
        let row: Vec<String> = (0..crop.width)
            .map(|x| {
                let [r, g, b, _] = crop.px(x, y);
                format!("{r:02x}{g:02x}{b:02x}")
            })
            .collect();
        println!("{}", row.join(" "));
    }
}

/// Parse `record` output back into frames.
fn load_recording(path: &str) -> Result<Vec<(i64, Crop)>, Box<dyn Error>> {
    let mut frames: Vec<(i64, Vec<Vec<[u8; 4]>>)> = Vec::new();
    for line in std::fs::read_to_string(path)?.lines() {
        if let Some(t) = line.strip_prefix("frame ") {
            frames.push((t.parse()?, Vec::new()));
        } else if let Some((_, rows)) = frames.last_mut() {
            let row = line
                .split(' ')
                .map(|p| Ok([u8::from_str_radix(&p[0..2], 16)?, u8::from_str_radix(&p[2..4], 16)?, u8::from_str_radix(&p[4..6], 16)?, 255]))
                .collect::<Result<Vec<_>, std::num::ParseIntError>>()?;
            rows.push(row);
        }
    }
    frames
        .into_iter()
        .map(|(t, rows)| {
            let height = u32::try_from(rows.len())?;
            let width = u32::try_from(rows.first().map_or(0, Vec::len))?;
            let rgba = rows.into_iter().flatten().flatten().collect();
            Ok((t, Crop { width, height, rgba }))
        })
        .collect()
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let num = |i: usize| -> Result<u32, Box<dyn Error>> { Ok(args.get(i).ok_or("missing argument")?.parse()?) };
    let mut rejected = Rejected::new();
    match args.first().map(String::as_str) {
        Some("replay") => {
            for (t, crop) in load_recording(args.get(1).ok_or("missing recording path")?)? {
                match read_exp(&crop) {
                    Ok(r) => println!("{t} {} [{}.{:02}%]", r.exp, r.pct_bp / 100, r.pct_bp % 100),
                    Err(e) => println!("{t} unreadable: {e}"),
                }
            }
            return Ok(());
        }
        Some("png") => {
            let path = args.get(1).ok_or("missing png path")?;
            let rect = Rect { x: num(2)?, y: num(3)?, w: num(4)?, h: num(5)? };
            let crop = Crop::from_image(&image::open(path)?.to_rgba8()).sub(rect).ok_or("rect outside image")?;
            collect_rejected(&crop, &mut rejected);
        }
        Some(mode @ ("live" | "record")) => {
            let record = mode == "record";
            let rect = Rect { x: num(1)?, y: num(2)?, w: num(3)?, h: num(4)? };
            let seconds = num(5)?;
            let monitor = windows_capture::monitor::Monitor::primary()?;
            let shared = Arc::new(Shared::default());
            *shared.regions.write().map_err(|_| "regions lock poisoned")? = Some(Regions {
                width: monitor.width()?,
                height: monitor.height()?,
                calibration: Calibration { exp: rect, map: rect, level: None },
            });
            let (tx, rx) = sync_channel(4);
            let control = capture::start(tx, shared)?;
            let deadline = Instant::now() + Duration::from_secs(seconds.into());
            let mut frames = 0;
            while Instant::now() < deadline {
                if let Ok(CaptureMsg::Regions { t, exp, .. }) = rx.recv_timeout(Duration::from_secs(1)) {
                    frames += 1;
                    if record {
                        print_pixels(t, &exp);
                    } else {
                        collect_rejected(&exp, &mut rejected);
                    }
                }
            }
            control.stop().map_err(|e| format!("{e:?}"))?;
            eprintln!("{frames} frames read");
            if record {
                return Ok(());
            }
        }
        _ => return Err("usage: glyph-collect record|live <x> <y> <w> <h> <seconds> | replay <file> | png <path> <x> <y> <w> <h>".into()),
    }
    for (what, count) in &rejected {
        println!("{count}x {what}");
    }
    println!("{} kind(s) of unrecognised glyph", rejected.len());
    Ok(())
}
