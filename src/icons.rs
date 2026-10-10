//! App icons: NSWorkspace iconForFile -> PNG cache -> gpui AssetSource.

#![allow(dead_code)]

use gpui_kit::{AssetSource, Result, SharedString};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::{Mutex, OnceLock};

fn icons_dir() -> PathBuf {
    let d = directories::ProjectDirs::from("no", "ogard", "tidegauge")
        .map(|p| p.cache_dir().join("icons"))
        .unwrap_or_else(|| PathBuf::from("/tmp/tidegauge-icons"));
    let _ = std::fs::create_dir_all(&d);
    d
}

fn refresh_icon_file(
    file: &std::path::Path,
    extract: impl FnOnce() -> Option<Vec<u8>>,
) -> Option<PathBuf> {
    let png = extract()?;
    std::fs::write(file, png).ok()?;
    Some(file.to_path_buf())
}

static ICON_KEYS: OnceLock<Mutex<HashMap<String, Option<PathBuf>>>> = OnceLock::new();
static PENDING_ICONS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
static ICON_WORKER: OnceLock<Option<SyncSender<String>>> = OnceLock::new();

/// Queue visible icons without blocking rendering or metric collection.
pub fn request_icon_for_bundle(bundle_path: &str) {
    let sender = ICON_WORKER.get_or_init(|| {
        let (sender, receiver) = sync_channel::<String>(32);
        std::thread::Builder::new()
            .name("tidegauge-icons".into())
            .spawn(move || {
                for path in receiver {
                    icon_file_for_bundle(&path);
                    PENDING_ICONS.get().unwrap().lock().unwrap().remove(&path);
                }
            })
            .ok()
            .map(|_| sender)
    });
    if let Some(sender) = sender {
        queue_icon(bundle_path, sender);
    }
}

fn queue_icon(bundle_path: &str, sender: &SyncSender<String>) {
    if ICON_KEYS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap()
        .contains_key(bundle_path)
    {
        return;
    }
    let mut pending = PENDING_ICONS
        .get_or_init(|| Mutex::new(HashSet::new()))
        .lock()
        .unwrap();
    if pending.insert(bundle_path.to_owned()) && sender.try_send(bundle_path.to_owned()).is_err() {
        pending.remove(bundle_path);
    }
}

/// Rendering only reads published paths; it never asks AppKit or the disk for an icon.
pub fn cached_icon_for_bundle(bundle_path: &str) -> Option<PathBuf> {
    ICON_KEYS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap()
        .get(bundle_path)
        .cloned()
        .flatten()
}

/// Map a bundle path (/Applications/Foo.app) to a cached PNG file path.
/// Extraction happens on the dedicated icon worker.
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
        if let Some(file) = m.get(bundle_path) {
            return file.clone();
        }
    }
    let file = icons_dir().join(&key);
    let result =
        objc2::rc::autoreleasepool(|_| refresh_icon_file(&file, || extract_icon_png(bundle_path)));
    // Remember failures too, rather than retrying expensive extraction every sample.
    map.lock()
        .unwrap()
        .insert(bundle_path.to_string(), result.clone());
    result
}

#[cfg(not(target_os = "macos"))]
pub fn icon_file_for_bundle(_: &str) -> Option<PathBuf> {
    None
}

#[cfg(target_os = "macos")]
fn extract_icon_png(bundle_path: &str) -> Option<Vec<u8>> {
    use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSImage, NSWorkspace};
    use objc2_foundation::{NSDictionary, NSString};
    unsafe {
        let ws = NSWorkspace::sharedWorkspace();
        let path = NSString::from_str(bundle_path);
        let img: objc2::rc::Retained<NSImage> = ws.iconForFile(&path);
        img.setSize(objc2_foundation::NSSize {
            width: 64.0,
            height: 64.0,
        });
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
        TideGaugeAssets {
            inner: gpui_kit::assets::AllAssets,
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_are_deduplicated_and_full_queues_can_retry() {
        let (sender, receiver) = sync_channel(1);
        let first = "/test/tidegauge-queued.app";
        let second = "/test/tidegauge-retry.app";
        queue_icon(first, &sender);
        queue_icon(first, &sender);
        queue_icon(second, &sender);
        assert_eq!(receiver.try_recv().unwrap(), first);
        assert!(receiver.try_recv().is_err());
        queue_icon(second, &sender);
        assert_eq!(receiver.try_recv().unwrap(), second);
        let mut pending = PENDING_ICONS.get().unwrap().lock().unwrap();
        pending.remove(first);
        pending.remove(second);
    }

    #[test]
    fn render_lookup_never_extracts_uncached_icons() {
        let bundle = "/nonexistent/tidegauge-render-test.app";
        assert_eq!(cached_icon_for_bundle(bundle), None);
        assert!(!ICON_KEYS
            .get()
            .unwrap()
            .lock()
            .unwrap()
            .contains_key(bundle));
    }

    #[test]
    fn render_lookup_returns_published_paths_and_failures() {
        let ready = "/test/tidegauge-ready.app";
        let missing = "/test/tidegauge-missing.app";
        let path = PathBuf::from("/test/icon.png");
        {
            let mut cache = ICON_KEYS
                .get_or_init(|| Mutex::new(HashMap::new()))
                .lock()
                .unwrap();
            cache.insert(ready.into(), Some(path.clone()));
            cache.insert(missing.into(), None);
        }
        assert_eq!(cached_icon_for_bundle(ready), Some(path));
        assert_eq!(cached_icon_for_bundle(missing), None);
    }

    #[test]
    fn uncached_icon_request_refreshes_existing_disk_file() {
        let file = std::env::temp_dir().join(format!(
            "tidegauge-icon-refresh-{}-{}.png",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&file, b"stale").unwrap();

        let refreshed = refresh_icon_file(&file, || Some(b"fresh".to_vec()));

        assert_eq!(refreshed.as_deref(), Some(file.as_path()));
        assert_eq!(std::fs::read(&file).unwrap(), b"fresh");
        std::fs::remove_file(file).unwrap();
    }
}
