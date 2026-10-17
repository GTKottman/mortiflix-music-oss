//! Stills on the nodes: pictures decoded off the UI thread (pure Rust `image`), shrunk to a small
//! texture once and kept. Only what is on screen asks for a still; a missing or broken file leaves the
//! node's empty frame, never an error.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};

/// Width the stills are shrunk to (a shot node is 216 px wide at 100 %, 324 at 150 %).
const THUMB_W: u32 = 384;
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
    Ready(egui::TextureHandle),
    Failed,
}

struct Inner {
    map: HashMap<PathBuf, Entry>,
    tx: Sender<(PathBuf, Result<egui::ColorImage, String>)>,
    rx: Receiver<(PathBuf, Result<egui::ColorImage, String>)>,
    loading: usize,
}

/// The still cache, kept in egui's memory for the node area.
#[derive(Clone)]
pub struct Stills(Arc<Mutex<Inner>>);

impl Default for Stills {
    fn default() -> Self {
        let (tx, rx) = channel();
        Self(Arc::new(Mutex::new(Inner { map: HashMap::new(), tx, rx, loading: 0 })))
    }
}

impl Stills {
    /// The cache for this egui context.
    pub fn of(ctx: &egui::Context) -> Stills {
        ctx.data_mut(|d| d.get_temp_mut_or_default::<Stills>(egui::Id::new("mfx-nodes-stills")).clone())
    }

    /// Take decoded pictures in as textures (call once per frame).
    pub fn poll(&self, ctx: &egui::Context) {
        let mut g = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let mut arrived = Vec::new();
        while let Ok(x) = g.rx.try_recv() {
            arrived.push(x);
        }
        for (path, r) in arrived {
            g.loading = g.loading.saturating_sub(1);
            let e = match r {
                Ok(img) => Entry::Ready(ctx.load_texture(format!("mfx-still:{}", path.display()), img, egui::TextureOptions::LINEAR)),
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
        match g.map.get(path) {
            Some(Entry::Ready(t)) => return Some(t.clone()),
            Some(Entry::Loading | Entry::Failed) => return None,
            None => {}
        }
        if g.loading >= MAX_LOADING {
            return None;
        }
        if g.map.len() >= MAX_KEPT {
            g.map.retain(|_, e| matches!(e, Entry::Loading));
        }
        g.map.insert(path.to_path_buf(), Entry::Loading);
        g.loading += 1;
        let tx = g.tx.clone();
        let p = path.to_path_buf();
        let spawned = std::thread::Builder::new().name("mfx-node-still".into()).spawn(move || {
            let r = std::panic::catch_unwind(|| decode(&p)).unwrap_or_else(|_| Err("the decoder failed".into()));
            let _ = tx.send((p, r));
        });
        if spawned.is_err() {
            g.loading = g.loading.saturating_sub(1);
            g.map.insert(path.to_path_buf(), Entry::Failed);
        }
        None
    }
}

/// Read and shrink one picture.
pub fn decode(path: &Path) -> Result<egui::ColorImage, String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.len() > MAX_FILE {
        return Err("not a picture file, or too big".into());
    }
    let reader = image::ImageReader::open(path).map_err(|e| e.to_string())?.with_guessed_format().map_err(|e| e.to_string())?;
    let (w, h) = reader.into_dimensions().map_err(|e| e.to_string())?;
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > MAX_PIXELS {
        return Err(format!("{w}x{h} is not a size we read"));
    }
    let img =
        image::ImageReader::open(path).map_err(|e| e.to_string())?.with_guessed_format().map_err(|e| e.to_string())?.decode().map_err(|e| e.to_string())?;
    let tw = THUMB_W.min(w);
    let th = (u64::from(h) * u64::from(tw) / u64::from(w)).clamp(1, 4096);
    let th = u32::try_from(th).unwrap_or(1);
    let small = image::imageops::thumbnail(&img.to_rgba8(), tw, th);
    let size = [usize::try_from(small.width()).unwrap_or(0), usize::try_from(small.height()).unwrap_or(0)];
    Ok(egui::ColorImage::from_rgba_unmultiplied(size, small.as_raw()))
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
}
