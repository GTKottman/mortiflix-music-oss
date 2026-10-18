//! Stills on the nodes: pictures decoded off the UI thread (pure Rust `image`), shrunk to a small
//! texture once and kept while it is on screen. Only what is on screen asks for a still, and a still
//! nobody drew for [`Stills::forget_unseen`]'s passes is let go (its node shows a wireframe while it
//! loads back in); a missing or broken file leaves the node's empty frame, never an error.
//!
//! The same cache draws the style frames of the Look stage and the chat's looks card: SVG frames are
//! rasterised in pure Rust (`resvg`, text in the system's fonts), and each picture's main colours are
//! read once (its palette, when the service has none). [`Stills::large`] keeps bigger textures for
//! frames shown large.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};

/// Width the stills are shrunk to (a shot node is 216 px wide at 100 %, 324 at 150 %).
const THUMB_W: u32 = 384;
/// Width of the large stills (a style frame on the Look stage, up to ~900 px on a big screen).
const LARGE_W: u32 = 1280;
/// Largest SVG file read (bytes).
const MAX_SVG: u64 = 16 * 1024 * 1024;
/// Most colours read from a picture.
const MAX_PALETTE: usize = 6;
/// Largest picture file read (bytes).
const MAX_FILE: u64 = 64 * 1024 * 1024;
/// Largest picture decoded (pixels): a hostile header can't ask for a huge allocation.
const MAX_PIXELS: u64 = 80_000_000;
/// Pictures decoding at once.
const MAX_LOADING: usize = 3;
/// Most stills kept.
const MAX_KEPT: usize = 600;

enum Entry {
    Loading,
    /// The texture and the picture's main colours.
    Ready(egui::TextureHandle, Vec<egui::Color32>),
    Failed,
}

type Decoded = Result<(egui::ColorImage, Vec<egui::Color32>), String>;

struct Inner {
    map: HashMap<PathBuf, Entry>,
    tx: Sender<(PathBuf, Decoded)>,
    rx: Receiver<(PathBuf, Decoded)>,
    loading: usize,
    /// Width the pictures are shrunk to.
    width: u32,
    /// The current egui pass (set by [`Stills::poll`]) and the pass each picture was last drawn in.
    pass: u64,
    seen: HashMap<PathBuf, u64>,
}

/// The still cache, kept in egui's memory for the node area.
#[derive(Clone)]
pub struct Stills(Arc<Mutex<Inner>>);

impl Default for Stills {
    fn default() -> Self {
        Self::with_width(THUMB_W)
    }
}

impl Stills {
    fn with_width(width: u32) -> Self {
        let (tx, rx) = channel();
        Self(Arc::new(Mutex::new(Inner { map: HashMap::new(), tx, rx, loading: 0, width, pass: 0, seen: HashMap::new() })))
    }

    /// The cache for this egui context.
    pub fn of(ctx: &egui::Context) -> Stills {
        ctx.data_mut(|d| d.get_temp_mut_or_default::<Stills>(egui::Id::new("mfx-nodes-stills")).clone())
    }

    /// The cache of large stills (style frames shown large) for this egui context.
    pub fn large(ctx: &egui::Context) -> Stills {
        ctx.data_mut(|d| d.get_temp_mut_or(egui::Id::new("mfx-large-stills"), Stills::with_width(LARGE_W)).clone())
    }

    /// The main colours of a picture already decoded (None while it loads or when it failed).
    pub fn palette(&self, path: &Path) -> Option<Vec<egui::Color32>> {
        let g = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        match g.map.get(path) {
            Some(Entry::Ready(_, p)) => Some(p.clone()),
            _ => None,
        }
    }

    /// Take decoded pictures in as textures (call once per frame).
    pub fn poll(&self, ctx: &egui::Context) {
        let mut g = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        g.pass = ctx.cumulative_pass_nr();
        let mut arrived = Vec::new();
        while let Ok(x) = g.rx.try_recv() {
            arrived.push(x);
        }
        for (path, r) in arrived {
            g.loading = g.loading.saturating_sub(1);
            let e = match r {
                Ok((img, palette)) => {
                    let name = format!("mfx-still:{}:{}", g.width, path.display());
                    Entry::Ready(ctx.load_texture(name, img, egui::TextureOptions::LINEAR), palette)
                }
                Err(e) => {
                    log::debug!("node still {}: {e}", path.display());
                    Entry::Failed
                }
            };
            g.map.insert(path, e);
        }
    }

    /// The texture for a picture file, starting its decode when it isn't known yet.
    pub fn get(&self, path: &Path) -> Option<egui::TextureHandle> {
        let mut g = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let pass = g.pass;
        match g.map.get(path) {
            Some(Entry::Ready(t, _)) => {
                let t = t.clone();
                g.seen.insert(path.to_path_buf(), pass);
                return Some(t);
            }
            Some(Entry::Loading | Entry::Failed) => return None,
            None => {}
        }
        if g.loading >= MAX_LOADING {
            return None;
        }
        if g.map.len() >= MAX_KEPT {
            g.map.retain(|_, e| matches!(e, Entry::Loading));
            g.seen.clear();
        }
        g.map.insert(path.to_path_buf(), Entry::Loading);
        g.loading += 1;
        let tx = g.tx.clone();
        let p = path.to_path_buf();
        let width = g.width;
        let spawned = std::thread::Builder::new().name("mfx-node-still".into()).spawn(move || {
            let r = std::panic::catch_unwind(|| decode_at(&p, width)).unwrap_or_else(|_| Err("the decoder failed".into()));
            let _ = tx.send((p, r));
        });
        if spawned.is_err() {
            g.loading = g.loading.saturating_sub(1);
            g.map.insert(path.to_path_buf(), Entry::Failed);
        }
        None
    }
}

impl Stills {
    /// True while a picture is on its way (decoding, or waiting for a free decoder): its node draws a
    /// wireframe until then. A picture that failed is not pending.
    pub fn pending(&self, path: &Path) -> bool {
        let g = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        !matches!(g.map.get(path), Some(Entry::Ready(..) | Entry::Failed))
    }

    /// Let go of the pictures nobody drew in the last `passes` egui passes (scrolled off screen): their
    /// textures are freed and they load again when they come back.
    pub fn forget_unseen(&self, passes: u64) {
        let mut g = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let cutoff = g.pass.saturating_sub(passes);
        let old: Vec<PathBuf> =
            g.map.iter().filter(|(p, e)| matches!(e, Entry::Ready(..)) && g.seen.get(*p).copied().unwrap_or(0) < cutoff).map(|(p, _)| p.clone()).collect();
        for p in old {
            g.map.remove(&p);
            g.seen.remove(&p);
        }
    }
}

/// Read and shrink one picture (a node's still).
pub fn decode(path: &Path) -> Result<egui::ColorImage, String> {
    decode_at(path, THUMB_W).map(|(img, _)| img)
}

/// Read one picture (PNG, JPEG, WebP or SVG) shrunk to `width`, and its main colours.
pub fn decode_at(path: &Path, width: u32) -> Decoded {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.len() > MAX_FILE {
        return Err("not a picture file, or too big".into());
    }
    let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
    let img = if matches!(ext.as_str(), "svg" | "svgz") {
        if meta.len() > MAX_SVG {
            return Err("the SVG is too big".into());
        }
        svg(path, width)?
    } else {
        raster(path, width)?
    };
    let palette = palette_of(&img);
    Ok((img, palette))
}

fn raster(path: &Path, width: u32) -> Result<egui::ColorImage, String> {
    let reader = image::ImageReader::open(path).map_err(|e| e.to_string())?.with_guessed_format().map_err(|e| e.to_string())?;
    let (w, h) = reader.into_dimensions().map_err(|e| e.to_string())?;
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > MAX_PIXELS {
        return Err(format!("{w}x{h} is not a size we read"));
    }
    let img =
        image::ImageReader::open(path).map_err(|e| e.to_string())?.with_guessed_format().map_err(|e| e.to_string())?.decode().map_err(|e| e.to_string())?;
    let tw = width.clamp(1, 4096).min(w);
    let th = (u64::from(h) * u64::from(tw) / u64::from(w)).clamp(1, 4096);
    let th = u32::try_from(th).unwrap_or(1);
    let small = image::imageops::thumbnail(&img.to_rgba8(), tw, th);
    let size = [usize::try_from(small.width()).unwrap_or(0), usize::try_from(small.height()).unwrap_or(0)];
    Ok(egui::ColorImage::from_rgba_unmultiplied(size, small.as_raw()))
}

/// The fonts SVG text is drawn with: the system's, loaded once; the generic families point at a face
/// that exists here (`sans-serif` → DejaVu Sans, Noto Sans, Arial…).
fn fonts() -> Arc<resvg::usvg::fontdb::Database> {
    static DB: std::sync::OnceLock<Arc<resvg::usvg::fontdb::Database>> = std::sync::OnceLock::new();
    DB.get_or_init(|| {
        let mut db = resvg::usvg::fontdb::Database::new();
        db.load_system_fonts();
        let has = |db: &resvg::usvg::fontdb::Database, name: &str| db.faces().any(|f| f.families.iter().any(|(n, _)| n.eq_ignore_ascii_case(name)));
        let first = |db: &resvg::usvg::fontdb::Database, names: &[&str]| names.iter().find(|n| has(db, n)).map(|n| (*n).to_string());
        if let Some(n) = first(&db, &["DejaVu Sans", "Noto Sans", "Liberation Sans", "Arial", "Helvetica", "Segoe UI"]) {
            db.set_sans_serif_family(n);
        }
        if let Some(n) = first(&db, &["DejaVu Serif", "Noto Serif", "Liberation Serif", "Times New Roman", "Georgia"]) {
            db.set_serif_family(n);
        }
        if let Some(n) = first(&db, &["DejaVu Sans Mono", "Noto Sans Mono", "Liberation Mono", "Consolas", "Menlo"]) {
            db.set_monospace_family(n);
        }
        Arc::new(db)
    })
    .clone()
}

/// Rasterise an SVG (pure Rust) to `width` px wide, keeping its shape.
fn svg(path: &Path, width: u32) -> Result<egui::ColorImage, String> {
    use resvg::{tiny_skia, usvg};
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    let mut opt = usvg::Options { fontdb: fonts(), ..usvg::Options::default() };
    opt.resources_dir = path.parent().map(Path::to_path_buf);
    let tree = usvg::Tree::from_data(&data, &opt).map_err(|e| e.to_string())?;
    let size = tree.size();
    let (sw, sh) = (size.width(), size.height());
    if !(sw.is_finite() && sh.is_finite() && sw > 0.0 && sh > 0.0) {
        return Err("the SVG has no size".into());
    }
    let tw = width.clamp(16, 4096);
    let th_f = (f64::from(sh) * f64::from(tw) / f64::from(sw)).round().clamp(1.0, 4096.0);
    let th = th_f as u32;
    let mut pixmap = tiny_skia::Pixmap::new(tw, th).ok_or("the SVG is too big to draw")?;
    let scale = tw as f32 / sw;
    resvg::render(&tree, tiny_skia::Transform::from_scale(scale, th as f32 / sh), &mut pixmap.as_mut());
    let size = [usize::try_from(tw).unwrap_or(0), usize::try_from(th).unwrap_or(0)];
    Ok(egui::ColorImage::from_rgba_premultiplied(size, pixmap.data()))
}

/// A picture's main colours, most used first (at most [`MAX_PALETTE`], each clearly different).
pub fn palette_of(img: &egui::ColorImage) -> Vec<egui::Color32> {
    let mut buckets: HashMap<u16, (u64, u64, u64, u64)> = HashMap::new();
    let step = (img.pixels.len() / 40_000).max(1);
    for p in img.pixels.iter().step_by(step).filter(|p| p.a() >= 128) {
        let key = (u16::from(p.r() >> 4) << 8) | (u16::from(p.g() >> 4) << 4) | u16::from(p.b() >> 4);
        let e = buckets.entry(key).or_insert((0, 0, 0, 0));
        e.0 += 1;
        e.1 += u64::from(p.r());
        e.2 += u64::from(p.g());
        e.3 += u64::from(p.b());
    }
    let mut list: Vec<(u64, egui::Color32)> = buckets
        .into_values()
        .filter(|b| b.0 > 0)
        .map(|(n, r, g, b)| {
            let avg = |x: u64| u8::try_from(x / n).unwrap_or(u8::MAX);
            (n, egui::Color32::from_rgb(avg(r), avg(g), avg(b)))
        })
        .collect();
    list.sort_by_key(|x| std::cmp::Reverse(x.0));
    let total: u64 = list.iter().map(|x| x.0).sum();
    let mut out: Vec<egui::Color32> = Vec::new();
    for (n, c) in list {
        // a colour on less than 0.5 % of the picture is a detail, not the palette
        if n.saturating_mul(200) < total {
            break;
        }
        let far = |o: &egui::Color32| u32::from(c.r().abs_diff(o.r())) + u32::from(c.g().abs_diff(o.g())) + u32::from(c.b().abs_diff(o.b())) > 60;
        if out.iter().all(far) {
            out.push(c);
            if out.len() >= MAX_PALETTE {
                break;
            }
        }
    }
    out
}

/// Where a media path from the service is on disk: absolute as is; relative to the graph's `base`,
/// else to `<studio>/state/<project>/`. None when nothing says where.
pub fn resolve(file: &str, base: &str, studio: &str, project: Option<&str>) -> Option<PathBuf> {
    if file.is_empty() || file.contains('\0') {
        return None;
    }
    let p = Path::new(file);
    if p.is_absolute() {
        return Some(p.to_path_buf());
    }
    // relative paths never climb out of their folder
    if p.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return None;
    }
    if !base.is_empty() {
        return Some(Path::new(base).join(p));
    }
    let project = project.filter(|id| !id.is_empty() && !id.contains(['/', '\\']) && *id != "..")?;
    (!studio.is_empty()).then(|| Path::new(studio).join("state").join(project).join(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_paths_and_refuses_climbing_out() {
        assert_eq!(resolve("canvas/a.png", "", "/st", Some("p1")), Some(PathBuf::from("/st/state/p1/canvas/a.png")));
        assert_eq!(resolve("canvas/a.png", "/base", "/st", Some("p1")), Some(PathBuf::from("/base/canvas/a.png")));
        assert_eq!(resolve("../../etc/passwd", "", "/st", Some("p1")), None);
        assert_eq!(resolve("a.png", "", "/st", Some("../x")), None);
        assert_eq!(resolve("a.png", "", "", Some("p1")), None);
        assert_eq!(resolve("", "/b", "/st", None), None);
        #[cfg(unix)]
        assert_eq!(resolve("/abs/a.png", "", "", None), Some(PathBuf::from("/abs/a.png")));
    }

    #[test]
    fn decodes_a_picture_and_refuses_junk() {
        let dir = std::env::temp_dir().join(format!("mfx-stills-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("a.png");
        image::RgbaImage::from_pixel(800, 450, image::Rgba([200, 30, 30, 255])).save(&png).unwrap();
        let img = decode(&png).unwrap();
        assert_eq!(img.size, [384, 216]);
        let junk = dir.join("b.png");
        std::fs::write(&junk, b"\x89PNG\r\n\x1a\nnot really").unwrap();
        assert!(decode(&junk).is_err());
        assert!(decode(&dir.join("missing.png")).is_err());
        assert!(decode(&dir).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Owner, 2026-10-18: stills scrolled off screen are let go, and are pending (wireframe) while they load back in.
    #[test]
    fn forgets_stills_nobody_drew_and_they_load_back_in() {
        let dir = std::env::temp_dir().join(format!("mfx-stills-forget-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("a.png");
        image::RgbaImage::from_pixel(64, 36, image::Rgba([20, 30, 200, 255])).save(&png).unwrap();
        let ctx = egui::Context::default();
        let stills = Stills::default();
        let pass = |ctx: &egui::Context| {
            ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta.clear();
            stills.poll(ctx);
        };
        let load = |ctx: &egui::Context| {
            for _ in 0..500 {
                pass(ctx);
                if stills.get(&png).is_some() {
                    return true;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            false
        };
        assert!(stills.get(&png).is_none());
        assert!(stills.pending(&png));
        assert!(load(&ctx));
        assert!(!stills.pending(&png));
        // drawn every pass: kept
        for _ in 0..10 {
            pass(&ctx);
            assert!(stills.get(&png).is_some());
        }
        stills.forget_unseen(5);
        assert!(!stills.pending(&png));
        // off screen for longer than that: let go, then pending again until it loads back in
        for _ in 0..10 {
            pass(&ctx);
        }
        stills.forget_unseen(5);
        assert!(stills.pending(&png));
        assert!(load(&ctx));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Style frames are often SVG (the AI writes them): drawn in pure Rust, with their colours read.
    #[test]
    fn draws_an_svg_frame_large_and_reads_its_colours() {
        let dir = std::env::temp_dir().join(format!("mfx-stills-svg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let svg = dir.join("sf01.svg");
        std::fs::write(
            &svg,
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="1920" height="1080"><rect width="1920" height="1080" fill="#0f172a"/><rect x="0" y="0" width="960" height="1080" fill="#f97316"/><text x="100" y="900" font-family="sans-serif" font-size="80" fill="#fff">SF01</text></svg>"##,
        )
        .unwrap();
        let (img, palette) = decode_at(&svg, 1280).unwrap();
        assert_eq!(img.size, [1280, 720]);
        let at = |x: usize, y: usize| img.pixels[y * 1280 + x];
        assert_eq!(at(100, 100), egui::Color32::from_rgb(0xf9, 0x73, 0x16));
        assert_eq!(at(1200, 100), egui::Color32::from_rgb(0x0f, 0x17, 0x2a));
        assert!(palette.contains(&egui::Color32::from_rgb(0xf9, 0x73, 0x16)) && palette.contains(&egui::Color32::from_rgb(0x0f, 0x17, 0x2a)), "{palette:?}");
        assert_eq!(decode(&svg).unwrap().size, [384, 216], "node stills stay small");
        std::fs::write(dir.join("bad.svg"), "<svg").unwrap();
        assert!(decode_at(&dir.join("bad.svg"), 1280).is_err());
        std::fs::write(dir.join("empty.svg"), r#"<svg xmlns="http://www.w3.org/2000/svg" width="0" height="0"/>"#).unwrap();
        assert!(decode_at(&dir.join("empty.svg"), 1280).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
