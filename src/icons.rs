//! App icons: NSWorkspace iconForFile -> PNG cache -> gpui AssetSource.

#![allow(dead_code)]

use gpui_kit::*;
use std::borrow::Cow;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

fn icons_dir() -> PathBuf {
    let d = directories::ProjectDirs::from("no", "ogard", "tidegauge")
        .map(|p| p.cache_dir().join("icons"))
        .unwrap_or_else(|| PathBuf::from("/tmp/tidegauge-icons"));
    let _ = std::fs::create_dir_all(&d);
    d
}

static ICON_KEYS: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();

/// Map a bundle path (/Applications/Foo.app) to a cached PNG file path.
/// Extraction happens on the sampler thread.
#[cfg(target_os = "macos")]
pub fn icon_file_for_bundle(bundle_path: &str) -> Option<PathBuf> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    bundle_path.hash(&mut h);
    let key = format!("{:016x}.png", h.finish());
    let map = ICON_KEYS.get_or_init(|| Mutex::new(HashMap::new()));
    {
        let m = map.lock().unwrap();
        if m.contains_key(&key) {
            return Some(icons_dir().join(&key));
        }
    }
    let png = extract_icon_png(bundle_path)?;
    let file = icons_dir().join(&key);
    if std::fs::write(&file, &png).is_ok() {
        map.lock().unwrap().insert(key.clone(), file.to_string_lossy().to_string());
        Some(file)
    } else {
        None
    }
}

#[cfg(not(target_os = "macos"))]
pub fn icon_file_for_bundle(_: &str) -> Option<PathBuf> {
    None
}

#[cfg(target_os = "macos")]
fn extract_icon_png(bundle_path: &str) -> Option<Vec<u8>> {
    use objc2_app_kit::{NSBitmapImageRep, NSBitmapImageFileType, NSImage, NSWorkspace};
    use objc2_foundation::{NSDictionary, NSString};
    unsafe {
        let ws = NSWorkspace::sharedWorkspace();
        let path = NSString::from_str(bundle_path);
        let img: objc2::rc::Retained<NSImage> = ws.iconForFile(&path);
        img.setSize(objc2_foundation::NSSize { width: 64.0, height: 64.0 });
        let tiff = img.TIFFRepresentation()?;
        let rep = NSBitmapImageRep::imageRepWithData(&tiff)?;
        let props = NSDictionary::new();
        let data = rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &props)?;
        Some(data.to_vec())
    }
}

/// Asset source: serves cached icon PNGs under "icon:", delegates the rest.
pub struct TideGaugeAssets {
    inner: gpui_kit::assets::AllAssets,
}

impl TideGaugeAssets {
    pub fn new() -> Self {
        TideGaugeAssets { inner: gpui_kit::assets::AllAssets }
    }
}

impl AssetSource for TideGaugeAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path == "tidegauge/logo.png" {
            return Ok(Some(Cow::Borrowed(include_bytes!("../assets/logo.png"))));
        }
        if let Some(rest) = path.strip_prefix("icon:") {
            let file = icons_dir().join(rest);
            if let Ok(bytes) = std::fs::read(&file) {
                return Ok(Some(Cow::Owned(bytes)));
            }
            return Ok(None);
        }
        self.inner.load(path)
    }
    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        self.inner.list(path)
    }
}
