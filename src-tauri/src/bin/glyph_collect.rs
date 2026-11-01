//! DEV ONLY (feature `dev-tools`, never shipped): tools for testing the screen readers.
//!
//! record: print a box's pixels as text each time they change, to test readers with.
//! replay: run the EXP reader over a recording and print what each frame read as.
//! scores: print how closely each glyph of each distinct line in a recording matched.
//! live / png: print the lines the reader can't read, with the closest characters.
//! maps: confirm maps in a recording of the map box (optionally a sub-rectangle of it) and
//! print what OCR reads for each and the map list name it matches, as the app does.
//!
//!   cargo run --manifest-path src-tauri/Cargo.toml --features dev-tools --bin glyph-collect -- record <x> <y> <w> <h> <seconds> > frames.txt
//!   cargo run --manifest-path src-tauri/Cargo.toml --features dev-tools --bin glyph-collect -- replay frames.txt
//!   cargo run --manifest-path src-tauri/Cargo.toml --features dev-tools --bin glyph-collect -- scores frames.txt
//!   cargo run --manifest-path src-tauri/Cargo.toml --features dev-tools --bin glyph-collect -- live <x> <y> <w> <h> <seconds>
//!   cargo run --manifest-path src-tauri/Cargo.toml --features dev-tools --bin glyph-collect -- png <path> <x> <y> <w> <h>
//!   cargo run --manifest-path src-tauri/Cargo.toml --features dev-tools --bin glyph-collect -- maps frames.txt [<x> <y> <w> <h>]
//!   cargo run --manifest-path src-tauri/Cargo.toml --features dev-tools --bin glyph-collect -- frame <path.png>
//!
//! frame: save one whole frame of the monitor as a lossless PNG, for measuring boxes.
//! meso-watch: read the inventory and quickslot each second (any 16:9) like the app does,
//! print each change, keep debug/meso-snapshot.png (the boxes at 3x, slots outlined green
//! when read, red when not) and .txt (each slot's count and item), and save a frame of
//! every count that jumps by more than JUMP between two frames as debug/meso-jump-<t>.png.
//!
//!   cargo run --manifest-path src-tauri/Cargo.toml --features dev-tools --bin glyph-collect -- meso-watch <seconds> [top-right|bottom-right|bottom-left]

use std::collections::BTreeMap;
use std::error::Error;
use std::sync::mpsc::sync_channel;
use std::sync::Arc;
use std::time::{Duration, Instant};

use classic_exp_tracker_lib::capture::{self, CaptureMsg, Regions, Shared};
use classic_exp_tracker_lib::geom::{Boxes, Crop, Rect, Scale};
use classic_exp_tracker_lib::read::exp::{read_exp, read_line, read_text_line, Ink};
use classic_exp_tracker_lib::read::count::{best_scores, Band};
use classic_exp_tracker_lib::read::font;
use classic_exp_tracker_lib::capture::LootCrops;
use classic_exp_tracker_lib::geom::{boxes_for, Corner};
use classic_exp_tracker_lib::items::ItemKind;
use classic_exp_tracker_lib::read::inventory::{
    inventory_slots, meso_field, parse_mesos, patch_distance, read_inventory, quickslot_layout, read_quickslot, read_slot, SlotRead, COUNT_HEIGHT, COUNT_TOP, ICON_PATCH, SLOT,
};
use classic_exp_tracker_lib::store::{ItemPatch, Store};
use classic_exp_tracker_lib::read::map_names::{match_name, NameMatch};
use classic_exp_tracker_lib::read::map_strip::{read_strip, Signature};
use classic_exp_tracker_lib::read::ocr::MapOcr;
use classic_exp_tracker_lib::tracker::{ConfirmOutcome, MapConfirmer};

/// A count change between two frames bigger than this is saved as a frame (meso-watch).
const JUMP: u32 = 20;

/// The dev app's items (tracker-dev.db), to name the stacks meso-watch reads.
fn dev_items() -> Result<Vec<ItemPatch>, Box<dyn Error>> {
    let appdata = std::env::var("APPDATA")?;
    let store = Store::open(&std::path::Path::new(&appdata).join("com.classicexp.tracker").join("tracker-dev.db"))?;
    Ok(store.item_patches()?)
}

fn item_name(items: &[ItemPatch], kind: ItemKind, slot: &SlotRead) -> String {
    items
        .iter()
        .filter(|i| i.kind == kind && patch_distance(&i.patch, &slot.patch.0) <= 3.0)
        .map(|i| i.name.clone().unwrap_or_else(|| format!("unnamed #{}", i.item_id)))
        .next()
        .unwrap_or_else(|| format!("new patch {:016x}", patch_hash(&slot.patch.0)))
}

/// What the app reads in the inventory and quickslot, one line per slot, and each
/// stack's count by item.
fn describe(crops: &LootCrops, items: &[ItemPatch]) -> (String, Vec<(String, u32)>) {
    let inv = read_inventory(&crops.inventory, &crops.inventory_at);
    let mut out = format!("  meso {:?}, tab {:?}\n", inv.meso, inv.tab);
    let mut counts = Vec::new();
    for (label, kind, crop, at, slots) in [
        ("inventory", ItemKind::Etc, &crops.inventory, &crops.inventory_at, inventory_slots().collect::<Vec<_>>()),
        ("quickslot", ItemKind::Use, &crops.quickslot, &crops.quickslot_at, quickslot_layout(&crops.quickslot, &crops.quickslot_at).to_vec()),
    ] {
        for (i, (x, y)) in slots.into_iter().enumerate() {
            match read_slot(crop, at, x, y) {
                Ok(Some(slot)) => {
                    let name = item_name(items, kind, &slot);
                    out += &format!("  {label} {i:>2}: {:>5} {name}\n", slot.count);
                    counts.push((format!("{label} {i} {name}"), slot.count));
                }
                Ok(None) => {}
                Err(_) => out += &format!("  {label} {i:>2}: UNREADABLE\n"),
            }
        }
    }
    (out, counts)
}

/// The inventory and quickslot side by side at 3x, each slot outlined (green: read, red:
/// unreadable, grey: empty) with its patch (white) and count rows (blue), as `<path>.png`,
/// and `summary` as `<path>.txt`.
fn snapshot(crops: &LootCrops, summary: &str, path: &str) -> Result<(), Box<dyn Error>> {
    const Z: u32 = 3;
    let (inv, quick) = (&crops.inventory, &crops.quickslot);
    let mut img = image::RgbaImage::from_pixel((inv.width + quick.width + 8) * Z, inv.height.max(quick.height) * Z, image::Rgba([40, 40, 40, 255]));
    let mut paste = |crop: &Crop, ox: u32| {
        for y in 0..crop.height * Z {
            for x in 0..crop.width * Z {
                img.put_pixel(ox + x, y, image::Rgba(crop.px(x / Z, y / Z)));
            }
        }
    };
    paste(inv, 0);
    let qx = (inv.width + 8) * Z;
    paste(quick, qx);
    // A native rect's outline where it lands in the crop.
    let mut outline = |ox: u32, at: &Scale, r: Rect, c: [u8; 4]| {
        let x = |n: u32| (at.ox + f64::from(n) * at.sx) as u32 * Z;
        let y = |n: u32| (at.oy + f64::from(n) * at.sy) as u32 * Z;
        let (x0, y0, x1, y1) = (ox + x(r.x), y(r.y), ox + x(r.x + r.w) - 1, y(r.y + r.h) - 1);
        for x in x0..=x1 {
            img.put_pixel(x, y0, image::Rgba(c));
            img.put_pixel(x, y1, image::Rgba(c));
        }
        for y in y0..=y1 {
            img.put_pixel(x0, y, image::Rgba(c));
            img.put_pixel(x1, y, image::Rgba(c));
        }
    };
    for (ox, crop, at, slots) in [(0, inv, &crops.inventory_at, inventory_slots().collect::<Vec<_>>()), (qx, quick, &crops.quickslot_at, quickslot_layout(quick, &crops.quickslot_at).to_vec())] {
        for (x, y) in slots {
            let colour = match read_slot(crop, at, x, y) {
                Ok(Some(_)) => [0, 220, 0, 255],
                Ok(None) => [120, 120, 120, 255],
                Err(_) => [255, 0, 0, 255],
            };
            outline(ox, at, Rect { x, y, w: SLOT, h: SLOT }, colour);
            outline(ox, at, Rect { x: x + ICON_PATCH.x, y: y + ICON_PATCH.y, ..ICON_PATCH }, [255, 255, 255, 255]);
            outline(ox, at, Rect { x, y: y + COUNT_TOP, w: SLOT, h: COUNT_HEIGHT }, [60, 140, 255, 255]);
        }
    }
    img.save(format!("{path}.png"))?;
    std::fs::write(format!("{path}.txt"), summary)?;
    Ok(())
}

/// The inventory corner named on the command line; top right when none is.
fn corner_arg(arg: Option<&String>) -> Result<Corner, Box<dyn Error>> {
    match arg.map(String::as_str) {
        None | Some("top-right") => Ok(Corner::TopRight),
        Some("bottom-right") => Ok(Corner::BottomRight),
        Some("bottom-left") => Ok(Corner::BottomLeft),
        Some(other) => Err(format!("unknown corner {other:?}").into()),
    }
}

/// A short fingerprint of a patch, to tell patches apart in the output.
fn patch_hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x100000001b3))
}

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
                    Ok(r) => {
                        let exp = r.exp_in_level(None).map_or(String::new(), |e| e.to_string());
                        println!("{t} {exp} [{}.{:02}%]", r.pct_bp() / 100, r.pct_bp() % 100);
                    }
                    Err(e) => println!("{t} unreadable: {e}"),
                }
            }
            return Ok(());
        }
        Some("scores") => {
            // Each distinct line once: every glyph's best character, its score and its
            // lead over the next closest (accepted at ≥ 0.75 with a lead of ≥ 0.03).
            let mut seen = std::collections::BTreeSet::new();
            for (t, crop) in load_recording(args.get(1).ok_or("missing recording path")?)? {
                let Ok(line) = read_line(&crop) else { continue };
                if !seen.insert(line.text()) {
                    continue;
                }
                println!("frame {t}: {:?} ({:.2} px per font pixel)", line.text(), line.font_px());
                for g in &line.glyphs {
                    let (best, score) = g.ranked.first().copied().unwrap_or(('-', f32::NAN));
                    let next = g.ranked.get(1).map_or(String::from("none"), |&(ch, s)| format!("{ch:?} {s:.3}"));
                    println!("  {best:?} {score:.3}  next {next}  lead {:.3}", score - g.ranked.get(1).map_or(0.0, |r| r.1));
                }
            }
            return Ok(());
        }
        Some("maps") => {
            let sub = match args.len() {
                2 => None,
                6 => Some(Rect { x: num(2)?, y: num(3)?, w: num(4)?, h: num(5)? }),
                _ => return Err("usage: glyph-collect maps <file> [<x> <y> <w> <h>]".into()),
            };
            let ocr = MapOcr::new()?;
            let mut confirmer = MapConfirmer::new();
            // Each map found so far: a look of it and what it was named.
            let mut seen: Vec<(Signature, String)> = Vec::new();
            for (t, frame) in load_recording(args.get(1).ok_or("missing recording path")?)? {
                let crop = match sub {
                    Some(r) => frame.sub(r).ok_or("sub-rectangle outside the recording")?,
                    None => frame,
                };
                let Ok(signature) = read_strip(&crop) else { continue };
                let ConfirmOutcome::Changed { signature, strips } = confirmer.feed(signature, crop) else { continue };
                if let Some((_, name)) = seen.iter().find(|(s, _)| s.matches(&signature)) {
                    println!("{t} known look: {name}");
                    continue;
                }
                let read = ocr.label(&strips)?;
                let name = match read.as_deref().map(match_name) {
                    Some(NameMatch::Listed(name)) => name.to_string(),
                    _ => "UNRECOGNISED".to_string(),
                };
                println!("{t} new look: OCR {read:?} -> {name}");
                seen.push((signature, name));
            }
            return Ok(());
        }
        Some("png") => {
            let path = args.get(1).ok_or("missing png path")?;
            let rect = Rect { x: num(2)?, y: num(3)?, w: num(4)?, h: num(5)? };
            let crop = Crop::from_image(&image::open(path)?.to_rgba8()).sub(rect).ok_or("rect outside image")?;
            collect_rejected(&crop, &mut rejected);
        }
        Some("read") => {
            // read <png> <x> <y> <w> <h> [meso]: what the EXP (or meso) font reader reads in a box.
            let path = args.get(1).ok_or("missing png path")?;
            let rect = Rect { x: num(2)?, y: num(3)?, w: num(4)?, h: num(5)? };
            let crop = Crop::from_image(&image::open(path)?.to_rgba8()).sub(rect).ok_or("rect outside image")?;
            let line = match args.get(6).map(String::as_str) {
                Some("meso") => read_text_line(&crop, &font::MESO, Ink::Dark),
                _ => read_line(&crop),
            }
            .map_err(|e| format!("{e}"))?;
            println!("{:?} ({} px line, {:.2} px per font pixel)", line.text(), line.text_height, line.font_px());
            for g in &line.glyphs {
                let near: Vec<String> = g.ranked.iter().take(3).map(|(ch, score)| format!("{ch:?} {score:.3}")).collect();
                println!("  column {}: {:?}  {}", g.x, g.ch, near.join(", "));
            }
            return Ok(());
        }
        Some("inventory") => {
            // inventory <png> [top-right|bottom-right|bottom-left]: what the inventory and
            // quickslot readers read in a 16:9 frame, with the inventory in that corner.
            let frame = Crop::from_image(&image::open(args.get(1).ok_or("missing png path")?)?.to_rgba8());
            let corner = corner_arg(args.get(2))?;
            let boxes = boxes_for(frame.width, frame.height, corner).ok_or("not 16:9")?;
            let inv = frame.sub(boxes.inventory).ok_or("frame too small")?;
            let read = read_inventory(&inv, &boxes.inventory_at);
            println!("found {}, meso {:?}, tab {:?}, scroll {:?}", read.found, read.meso, read.tab, read.scroll);
            // Each stack with the dev database's two closest Etc items, to judge patches by.
            let items = dev_items()?;
            for s in read.slots.iter().flatten() {
                let mut near: Vec<(f32, String)> = items
                    .iter()
                    .filter(|i| i.kind == ItemKind::Etc)
                    .map(|i| (patch_distance(&i.patch, &s.patch.0), format!("#{} {}", i.item_id, i.name.as_deref().unwrap_or("unnamed"))))
                    .collect();
                near.sort_by(|a, b| a.0.total_cmp(&b.0));
                let near: Vec<String> = near.iter().take(2).map(|(d, name)| format!("{d:.1} {name}")).collect();
                println!("  {:>4} x patch {:016x}  {}", s.count, patch_hash(&s.patch.0), near.join(" | "));
            }
            if read.slots.is_none() {
                println!("  slots unreadable");
            }
            let quick = frame.sub(boxes.quickslot).ok_or("frame too small")?;
            println!("quickslot {:?}", read_quickslot(&quick, &boxes.quickslot_at).map(|v| v.iter().map(|s| (s.count, patch_hash(&s.patch.0))).collect::<Vec<_>>()));
            return Ok(());
        }
        Some("meso-watch") => {
            let seconds = num(1)?;
            let monitor = windows_capture::monitor::Monitor::primary()?;
            let (w, h) = (monitor.width()?, monitor.height()?);
            let boxes = boxes_for(w, h, corner_arg(args.get(2))?).ok_or("the screen isn't 16:9")?;
            let items = dev_items()?;
            let shared = Arc::new(Shared::default());
            shared.loot_enabled.store(true, std::sync::atomic::Ordering::Release);
            *shared.regions.write().map_err(|_| "regions lock poisoned")? = Some(Regions { width: w, height: h, boxes });
            let (tx, rx) = sync_channel(4);
            let control = capture::start(tx, shared)?;
            std::fs::create_dir_all("debug")?;
            let deadline = Instant::now() + Duration::from_secs(seconds.into());
            let mut last: Option<String> = None;
            let mut last_counts: Vec<(String, u32)> = Vec::new();
            while Instant::now() < deadline {
                let Ok(CaptureMsg::Regions { t, loot: Some(crops), .. }) = rx.recv_timeout(Duration::from_secs(2)) else { continue };
                let (summary, counts) = describe(&crops, &items);
                if last.as_ref() != Some(&summary) {
                    println!("{t}\n{summary}");
                    snapshot(&crops, &summary, "debug/meso-snapshot")?;
                    for (name, count) in &counts {
                        if let Some((_, before)) = last_counts.iter().find(|(n, _)| n == name)
                            && before.abs_diff(*count) > JUMP
                        {
                            println!("JUMP {name}: {before} -> {count}");
                            snapshot(&crops, &summary, &format!("debug/meso-jump-{t}"))?;
                        }
                    }
                    last = Some(summary);
                    last_counts = counts;
                }
            }
            control.stop().map_err(|e| format!("{e:?}"))?;
            return Ok(());
        }
        Some("meso-glyphs") => {
            // meso-glyphs <seconds> [corner]: each new reading of the meso total, glyph by
            // glyph (column, width, cut from touching glyphs, closest characters), with the
            // field saved as debug/meso-field-<t>.png.
            let seconds = num(1)?;
            let monitor = windows_capture::monitor::Monitor::primary()?;
            let (w, h) = (monitor.width()?, monitor.height()?);
            let boxes = boxes_for(w, h, corner_arg(args.get(2))?).ok_or("the screen isn't 16:9")?;
            let shared = Arc::new(Shared::default());
            shared.loot_enabled.store(true, std::sync::atomic::Ordering::Release);
            *shared.regions.write().map_err(|_| "regions lock poisoned")? = Some(Regions { width: w, height: h, boxes });
            let (tx, rx) = sync_channel(4);
            let control = capture::start(tx, shared)?;
            std::fs::create_dir_all("debug")?;
            let deadline = Instant::now() + Duration::from_secs(seconds.into());
            let mut last: Option<String> = None;
            while Instant::now() < deadline {
                let Ok(CaptureMsg::Regions { t, loot: Some(crops), .. }) = rx.recv_timeout(Duration::from_secs(2)) else { continue };
                let Some(field) = meso_field(&crops.inventory, &crops.inventory_at) else { continue };
                let line = match read_text_line(&field, &font::MESO, Ink::Dark) {
                    Ok(line) => line,
                    Err(e) => {
                        println!("{t} {e}");
                        continue;
                    }
                };
                if last.as_deref() == Some(line.text().as_str()) {
                    continue;
                }
                println!("{t} {:?} ({} px rows)", line.text(), line.text_height);
                for g in &line.glyphs {
                    let near: Vec<String> = g.ranked.iter().take(3).map(|(ch, s)| format!("{ch:?} {s:.3}")).collect();
                    println!("  x {:>3} w {:>2}{} {:?}  {}", g.x, g.width, if g.split { " split" } else { "" }, g.ch, near.join(", "));
                }
                field.to_image().save(format!("debug/meso-field-{t}.png"))?;
                last = Some(line.text());
            }
            control.stop().map_err(|e| format!("{e:?}"))?;
            return Ok(());
        }
        Some("meso-png") => {
            // meso-png <png>...: what the meso reader reads in saved meso fields (meso-glyphs).
            for path in &args[1..] {
                let field = Crop::from_image(&image::open(path)?.to_rgba8());
                let text = read_text_line(&field, &font::MESO, Ink::Dark).map(|line| line.text());
                println!("{path} {text:?} {:?}", text.as_deref().ok().and_then(parse_mesos));
            }
            return Ok(());
        }
        Some("count-scores") => {
            // count-scores <png>: each slot's best digit score per column (16:9 frame, the
            // inventory top right).
            let frame = Crop::from_image(&image::open(args.get(1).ok_or("missing png path")?)?.to_rgba8());
            let boxes = boxes_for(frame.width, frame.height, Corner::TopRight).ok_or("not 16:9")?;
            let inv = frame.sub(boxes.inventory).ok_or("frame too small")?;
            let quick = frame.sub(boxes.quickslot).ok_or("frame too small")?;
            for (crop, at, slots) in [(&inv, &boxes.inventory_at, inventory_slots().collect::<Vec<_>>()), (&quick, &boxes.quickslot_at, quickslot_layout(&quick, &boxes.quickslot_at).to_vec())] {
                for (x, y) in slots {
                    let Some((band, left, top)) = at.around(crop, Rect { x, y: y + COUNT_TOP, w: SLOT, h: COUNT_HEIGHT }) else { continue };
                    let scores = best_scores(&Band::new(&band, left, top), at.font_px());
                    println!("{}", scores.iter().map(|(x, ch, s)| format!("{x:.0}{ch}{s:.2}")).collect::<Vec<_>>().join(" "));
                }
            }
            return Ok(());
        }
        Some("frame") => {
            let path = args.get(1).ok_or("missing png path")?;
            let monitor = windows_capture::monitor::Monitor::primary()?;
            let whole = Rect { x: 0, y: 0, w: monitor.width()?, h: monitor.height()? };
            let shared = Arc::new(Shared::default());
            *shared.regions.write().map_err(|_| "regions lock poisoned")? = Some(Regions {
                width: whole.w,
                height: whole.h,
                boxes: Boxes { exp: whole, map: whole, level: whole, bar: whole, inventory: whole, quickslot: whole, inventory_at: Scale::NATIVE, quickslot_at: Scale::NATIVE },
            });
            let (tx, rx) = sync_channel(4);
            let control = capture::start(tx, shared)?;
            let frame = loop {
                match rx.recv_timeout(Duration::from_secs(5)) {
                    Ok(CaptureMsg::Regions { exp, .. }) => break exp,
                    Ok(_) => continue,
                    Err(e) => return Err(format!("no frame: {e}").into()),
                }
            };
            control.stop().map_err(|e| format!("{e:?}"))?;
            frame.to_image().save(path)?;
            println!("saved {}x{} frame to {path}", frame.width, frame.height);
            return Ok(());
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
                boxes: Boxes { exp: rect, map: rect, level: rect, bar: rect, inventory: rect, quickslot: rect, inventory_at: Scale::NATIVE, quickslot_at: Scale::NATIVE },
            });
            let (tx, rx) = sync_channel(4);
            let control = capture::start(tx, shared)?;
            let deadline = Instant::now() + Duration::from_secs(seconds.into());
            let mut frames = 0;
            let mut last: Option<Crop> = None;
            while Instant::now() < deadline {
                if let Ok(CaptureMsg::Regions { t, exp, .. }) = rx.recv_timeout(Duration::from_secs(1)) {
                    frames += 1;
                    if record {
                        // An unchanged box adds nothing to a recording.
                        if last.as_ref() != Some(&exp) {
                            print_pixels(t, &exp);
                            last = Some(exp);
                        }
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
        _ => return Err("usage: glyph-collect record|live <x> <y> <w> <h> <seconds> | replay|scores|maps <file> | png <path> <x> <y> <w> <h>".into()),
    }
    for (what, count) in &rejected {
        println!("{count}x {what}");
    }
    println!("{} kind(s) of unrecognised glyph", rejected.len());
    Ok(())
}
