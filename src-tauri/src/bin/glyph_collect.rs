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
