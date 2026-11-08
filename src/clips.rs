//! Clipboard history: what the user copies, new screenshots, and files dropped on the notch.
//! Kept in memory only, so clips never touch the disk and vanish when the app quits.

use std::ffi::{CString, c_char, c_void};
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::AnyThread;
use objc2_app_kit::{
    NSBitmapImageFileType, NSBitmapImageRep, NSImage, NSPasteboard, NSPasteboardItem, NSPasteboardTypeFileURL,
    NSPasteboardTypePNG, NSPasteboardTypeString, NSPasteboardTypeTIFF, NSWorkspace,
};
use objc2_foundation::{NSArray, NSData, NSDictionary, NSPoint, NSRect, NSSize, NSString, NSURL, NSUserDefaults, ns_string};

const MAX_CLIPS: usize = 12;
/// Longest side of a stored thumbnail, in points (cards show them at about half that).
const THUMB_SIZE: f64 = 240.0;
const IMAGE_EXTENSIONS: [&str; 8] = ["png", "jpg", "jpeg", "heic", "gif", "tiff", "tif", "webp"];
/// Marker types password managers and similar apps put on copies that must not be recorded
/// (see nspasteboard.org).
const PRIVATE_TYPES: [&str; 2] = ["org.nspasteboard.ConcealedType", "org.nspasteboard.TransientType"];

unsafe extern "C" {
    fn getxattr(path: *const c_char, name: *const c_char, value: *mut c_void, size: usize, position: u32, options: i32) -> isize;
}

#[derive(Clone)]
pub enum Kind {
    Text(String),
    /// PNG data, converted from TIFF when that is all the pasteboard had.
    Image(Retained<NSData>),
    File(PathBuf),
}

#[derive(Clone)]
pub struct Clip {
    pub kind: Kind,
    /// Downscaled preview for images and files; `None` for text.
    pub thumb: Option<Retained<NSImage>>,
}

impl Clip {
    fn new(kind: Kind) -> Self {
        let thumb = match &kind {
            Kind::Text(_) => None,
            Kind::Image(data) => NSImage::initWithData(NSImage::alloc(), data).map(|image| thumbnail(&image)),
            Kind::File(path) => Some(file_preview(path)),
        };
        Clip { kind, thumb }
    }

    /// A fresh pasteboard item holding this clip, for copying or for a drag.
    pub fn pasteboard_item(&self) -> Retained<NSPasteboardItem> {
        let item = NSPasteboardItem::new();
        unsafe {
            match &self.kind {
                Kind::Text(text) => item.setString_forType(&NSString::from_str(text), NSPasteboardTypeString),
                Kind::Image(data) => item.setData_forType(data, NSPasteboardTypePNG),
                Kind::File(path) => {
                    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
                    let url = url.absoluteString().unwrap_or_default();
                    item.setString_forType(&url, NSPasteboardTypeFileURL)
                }
            };
        }
        item
    }

    /// Puts the clip back on the clipboard. The history notices the change on its next poll
    /// and moves this clip to the front.
    pub fn copy(&self) {
        let pasteboard = NSPasteboard::generalPasteboard();
        pasteboard.clearContents();
        pasteboard.writeObjects(&NSArray::from_retained_slice(&[ProtocolObject::from_retained(self.pasteboard_item())]));
    }

    fn same_as(&self, kind: &Kind) -> bool {
        match (&self.kind, kind) {
            (Kind::Text(a), Kind::Text(b)) => a == b,
            (Kind::Image(a), Kind::Image(b)) => a.isEqualToData(b),
            (Kind::File(a), Kind::File(b)) => a == b,
            _ => false,
        }
    }
}

/// Newest-first list of clips, filled by polling the general pasteboard and the screenshot
/// folder.
pub struct ClipHistory {
    clips: Vec<Clip>,
    change_count: isize,
    shots_dir: PathBuf,
    /// The folder's mtime when last listed; it changes whenever a file is added.
    shots_dir_mtime: Option<SystemTime>,
    /// Screenshots older than this were already seen (or predate the app).
    shots_since: SystemTime,
}

impl Default for ClipHistory {
    fn default() -> Self {
        let shots_dir = screenshot_dir();
        ClipHistory {
            clips: Vec::new(),
            // Whatever is on the clipboard at launch becomes the first clip.
            change_count: isize::MIN,
            // Listing Desktop needs the user's permission, so wait until something lands there.
            shots_dir_mtime: modified(&shots_dir),
            shots_dir,
            shots_since: SystemTime::now(),
        }
    }
}

impl ClipHistory {
    pub fn clips(&self) -> &[Clip] {
        &self.clips
    }

    /// Records a new clipboard copy and any new screenshots. Returns whether anything changed.
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        let pasteboard = NSPasteboard::generalPasteboard();
        let count = pasteboard.changeCount();
        if count != self.change_count {
            self.change_count = count;
            for kind in read_pasteboard(&pasteboard) {
                self.push(kind);
                changed = true;
            }
        }
        for path in self.new_screenshots() {
            self.push(Kind::File(path));
            changed = true;
        }
        changed
    }

    pub fn add_files(&mut self, paths: Vec<PathBuf>) {
        for path in paths {
            self.push(Kind::File(path));
        }
    }

    pub fn clear(&mut self) {
        self.clips.clear();
    }

    /// Moves an equal clip to the front (keeping its thumbnail), or adds a new one.
    fn push(&mut self, kind: Kind) {
        let clip = match self.clips.iter().position(|c| c.same_as(&kind)) {
            Some(i) => self.clips.remove(i),
            None => Clip::new(kind),
        };
        self.clips.insert(0, clip);
        self.clips.truncate(MAX_CLIPS);
    }

    fn new_screenshots(&mut self) -> Vec<PathBuf> {
        let mtime = modified(&self.shots_dir);
        if mtime.is_none() || mtime == self.shots_dir_mtime {
            return Vec::new();
        }
        self.shots_dir_mtime = mtime;
        let since = std::mem::replace(&mut self.shots_since, SystemTime::now());
        let Ok(entries) = fs::read_dir(&self.shots_dir) else { return Vec::new() };
        let mut shots: Vec<(SystemTime, PathBuf)> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| is_screen_capture(path))
            .filter_map(|path| Some((modified(&path)?, path)))
            .filter(|(time, _)| *time >= since)
            .collect();
        shots.sort();
        shots.into_iter().map(|(_, path)| path).collect()
    }
}

/// File paths on a pasteboard (Finder copies and drags carry one file URL per item).
pub fn file_paths(pasteboard: &NSPasteboard) -> Vec<PathBuf> {
    let items = pasteboard.pasteboardItems().unwrap_or_default();
    items
        .iter()
        .filter_map(|item| item.stringForType(unsafe { NSPasteboardTypeFileURL }))
        .filter_map(|url| NSURL::URLWithString(&url)?.path())
        .map(|path| PathBuf::from(path.to_string()))
        .collect()
}

/// What a copy put on the pasteboard, most specific first: files, else an image, else text.
fn read_pasteboard(pasteboard: &NSPasteboard) -> Vec<Kind> {
    let types = pasteboard.types().unwrap_or_default();
    if types.iter().any(|t| PRIVATE_TYPES.contains(&t.to_string().as_str())) {
        return Vec::new();
    }
    let files = file_paths(pasteboard);
    if !files.is_empty() {
        return files.into_iter().map(Kind::File).collect();
    }
    if let Some(png) = pasteboard.dataForType(unsafe { NSPasteboardTypePNG }) {
        return vec![Kind::Image(png)];
    }
    if let Some(png) = pasteboard.dataForType(unsafe { NSPasteboardTypeTIFF }).and_then(|tiff| to_png(&tiff)) {
        return vec![Kind::Image(png)];
    }
    match pasteboard.stringForType(unsafe { NSPasteboardTypeString }) {
        Some(text) if !text.to_string().trim().is_empty() => vec![Kind::Text(text.to_string())],
        _ => Vec::new(),
    }
}

/// TIFF from the pasteboard is uncompressed; PNG keeps a full-screen copy a few MB at most.
fn to_png(tiff: &NSData) -> Option<Retained<NSData>> {
    let rep = NSBitmapImageRep::imageRepWithData(tiff)?;
    unsafe { rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new()) }
}

fn file_preview(path: &Path) -> Retained<NSImage> {
    let path_string = NSString::from_str(&path.to_string_lossy());
    let is_image = path
        .extension()
        .is_some_and(|ext| IMAGE_EXTENSIONS.contains(&ext.to_string_lossy().to_lowercase().as_str()));
    let image = if is_image { NSImage::initWithContentsOfFile(NSImage::alloc(), &path_string) } else { None };
    match image {
        Some(image) => thumbnail(&image),
        None => NSWorkspace::sharedWorkspace().iconForFile(&path_string),
    }
}

/// Small copy of `image`, so a full-size screenshot isn't kept decoded in memory.
fn thumbnail(image: &NSImage) -> Retained<NSImage> {
    let size = image.size();
    let scale = (THUMB_SIZE / size.width.max(size.height).max(1.0)).min(1.0);
    let target = NSSize::new((size.width * scale).round(), (size.height * scale).round());
    let thumb = NSImage::initWithSize(NSImage::alloc(), target);
    #[allow(deprecated)]
    {
        thumb.lockFocus();
        image.drawInRect(NSRect::new(NSPoint::ZERO, target));
        thumb.unlockFocus();
    }
    thumb
}

/// Where macOS saves screenshots: `defaults write com.apple.screencapture location`, else Desktop.
fn screenshot_dir() -> PathBuf {
    let custom = NSUserDefaults::standardUserDefaults()
        .persistentDomainForName(ns_string!("com.apple.screencapture"))
        .and_then(|domain| domain.objectForKey(ns_string!("location")))
        .and_then(|location| location.downcast::<NSString>().ok())
        .map(|location| PathBuf::from(location.stringByExpandingTildeInPath().to_string()));
    custom.unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("Desktop"))
}

/// macOS tags screenshots and screen recordings with this attribute; the name check covers
/// files copied from elsewhere that lost it.
fn is_screen_capture(path: &Path) -> bool {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if name.starts_with('.') {
        return false;
    }
    if name.starts_with("Screenshot") || name.starts_with("Screen Recording") {
        return true;
    }
    let (Ok(path), Ok(attr)) = (CString::new(path.as_os_str().as_bytes()), CString::new("com.apple.metadata:kMDItemIsScreenCapture")) else {
        return false;
    };
    unsafe { getxattr(path.as_ptr(), attr.as_ptr(), std::ptr::null_mut(), 0, 0, 0) >= 0 }
}

fn modified(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}
