use core::slice;
use std::ffi::{CStr, c_char, c_void};
use std::path::PathBuf;

use objc2::{class, msg_send, rc::Retained, runtime::AnyObject};
use smallvec::SmallVec;
use strum::IntoEnumIterator as _;

use crate::{
    ns::{NSInteger, NSUInteger, array_iter, id, nil},
    ns_string,
};
use gpui::{
    ClipboardEntry, ClipboardItem, ClipboardString, ExternalPaths, Image, ImageFormat, hash,
};

pub struct Pasteboard {
    inner: Retained<AnyObject>,
    text_hash_type: Retained<AnyObject>,
    metadata_type: Retained<AnyObject>,
}

impl Pasteboard {
    pub fn general() -> Self {
        unsafe { Self::new(msg_send![class!(NSPasteboard), generalPasteboard]) }
    }

    pub fn find() -> Self {
        unsafe {
            Self::new(msg_send![
                class!(NSPasteboard),
                pasteboardWithName: NSPasteboardNameFind
            ])
        }
    }

    unsafe fn new(inner: id) -> Self {
        // These constructors return autoreleased objects, but a Pasteboard can
        // outlive the autorelease pool in which it was created.
        Self {
            inner: unsafe { Retained::retain(inner).unwrap() },
            text_hash_type: unsafe { Retained::retain(ns_string("zed-text-hash")).unwrap() },
            metadata_type: unsafe { Retained::retain(ns_string("zed-metadata")).unwrap() },
        }
    }

    pub fn read(&self) -> Option<ClipboardItem> {
        unsafe {
            // Check for file paths first
            let filenames: id = msg_send![&*self.inner, propertyListForType: NSFilenamesPboardType];
            if filenames != nil {
                let mut paths = SmallVec::new();
                for file in array_iter(filenames) {
                    let f: *const c_char = msg_send![file, UTF8String];
                    let path = CStr::from_ptr(f).to_string_lossy().into_owned();
                    paths.push(PathBuf::from(path));
                }
                if !paths.is_empty() {
                    let mut entries = vec![ClipboardEntry::ExternalPaths(ExternalPaths(paths))];

                    // Also include the string representation so text editors can
                    // paste the path as text.
                    if let Some(string_item) = self.read_string_from_pasteboard() {
                        entries.push(string_item);
                    }

                    return Some(ClipboardItem { entries });
                }
            }

            // Next, check for a plain string.
            if let Some(string_entry) = self.read_string_from_pasteboard() {
                return Some(ClipboardItem {
                    entries: vec![string_entry],
                });
            }

            // Finally, try the various supported image types.
            for format in ImageFormat::iter() {
                if let Some(item) = self.read_image(format) {
                    return Some(item);
                }
            }
        }

        None
    }

    fn read_image(&self, format: ImageFormat) -> Option<ClipboardItem> {
        let ut_type: UTType = format.into();

        unsafe {
            let types: id = msg_send![&*self.inner, types];
            let contains: bool =
                !types.is_null() && msg_send![types, containsObject: ut_type.inner()];
            if contains {
                self.data_for_type(ut_type.inner_mut()).map(|bytes| {
                    let bytes = bytes.to_vec();
                    let id = hash(&bytes);

                    ClipboardItem {
                        entries: vec![ClipboardEntry::Image(Image { format, bytes, id })],
                    }
                })
            } else {
                None
            }
        }
    }

    unsafe fn read_string_from_pasteboard(&self) -> Option<ClipboardEntry> {
        unsafe {
            let pasteboard_types: id = msg_send![&*self.inner, types];
            let string_type: id = ns_string("public.utf8-plain-text");

            let contains: bool = !pasteboard_types.is_null()
                && msg_send![pasteboard_types, containsObject: string_type];
            if !contains {
                return None;
            }

            let text_bytes = self.data_for_type(string_type)?;

            let text = String::from_utf8_lossy(&text_bytes).to_string();
            let metadata = self
                .data_for_type(Retained::as_ptr(&self.text_hash_type).cast_mut())
                .and_then(|hash_bytes| {
                    let hash_bytes = hash_bytes.as_slice().try_into().ok()?;
                    let hash = u64::from_be_bytes(hash_bytes);
                    let metadata =
                        self.data_for_type(Retained::as_ptr(&self.metadata_type).cast_mut())?;

                    if hash == ClipboardString::text_hash(&text) {
                        String::from_utf8(metadata).ok()
                    } else {
                        None
                    }
                });

            Some(ClipboardEntry::String(ClipboardString { text, metadata }))
        }
    }

    unsafe fn data_for_type(&self, kind: id) -> Option<Vec<u8>> {
        unsafe {
            let data: id = msg_send![&*self.inner, dataForType: kind];
            if data == nil {
                return None;
            }
            let bytes: *const c_void = msg_send![data, bytes];
            if bytes.is_null() {
                Some(Vec::new())
            } else {
                let length: NSUInteger = msg_send![data, length];
                Some(slice::from_raw_parts(bytes.cast::<u8>(), length).to_vec())
            }
        }
    }

    pub fn write(&self, item: ClipboardItem) {
        unsafe {
            match item.entries.as_slice() {
                [] => {
                    // Writing an empty list of entries just clears the clipboard.
                    let _: NSInteger = msg_send![&*self.inner, clearContents];
                }
                [ClipboardEntry::String(string)] => {
                    self.write_plaintext(string);
                }
                [ClipboardEntry::Image(image)] => {
                    self.write_image(image);
                }
                [ClipboardEntry::ExternalPaths(_)] => {}
                _ => {
                    // Agus NB: We're currently only writing string entries to the clipboard when we have more than one.
                    //
                    // This was the existing behavior before I refactored the outer clipboard code:
                    // https://github.com/zed-industries/zed/blob/65f7412a0265552b06ce122655369d6cc7381dd6/crates/gpui/src/platform/mac/platform.rs#L1060-L1110
                    //
                    // Note how `any_images` is always `false`. We should fix that, but that's orthogonal to the refactor.

                    let mut combined = ClipboardString {
                        text: String::new(),
                        metadata: None,
                    };

                    for entry in item.entries {
                        match entry {
                            ClipboardEntry::String(text) => {
                                combined.text.push_str(&text.text());
                                if combined.metadata.is_none() {
                                    combined.metadata = text.metadata;
                                }
                            }
                            _ => {}
                        }
                    }

                    self.write_plaintext(&combined);
                }
            }
        }
    }

    fn write_plaintext(&self, string: &ClipboardString) {
        unsafe {
            let _: NSInteger = msg_send![&*self.inner, clearContents];

            let text_bytes = ns_data(string.text.as_ptr(), string.text.len());
            let _: bool =
                msg_send![&*self.inner, setData: text_bytes, forType: NSPasteboardTypeString];

            if let Some(metadata) = string.metadata.as_ref() {
                let hash_bytes = ClipboardString::text_hash(&string.text).to_be_bytes();
                let hash_bytes = ns_data(hash_bytes.as_ptr(), hash_bytes.len());
                let _: bool =
                    msg_send![&*self.inner, setData: hash_bytes, forType: &*self.text_hash_type];

                let metadata_bytes = ns_data(metadata.as_ptr(), metadata.len());
                let _: bool =
                    msg_send![&*self.inner, setData: metadata_bytes, forType: &*self.metadata_type];
            }
        }
    }

    unsafe fn write_image(&self, image: &Image) {
        unsafe {
            let _: NSInteger = msg_send![&*self.inner, clearContents];

            let bytes = ns_data(image.bytes.as_ptr(), image.bytes.len());

            let ut_type: UTType = image.format.into();
            let _: bool = msg_send![&*self.inner, setData: bytes, forType: ut_type.inner_mut()];
        }
    }
}

/// Returns an autoreleased `NSData` copy of `bytes`.
unsafe fn ns_data(bytes: *const u8, length: usize) -> id {
    unsafe { msg_send![class!(NSData), dataWithBytes: bytes as *const c_void, length: length] }
}

#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {
    /// [Apple's documentation](https://developer.apple.com/documentation/appkit/nspasteboardnamefind?language=objc)
    pub static NSPasteboardNameFind: id;
    pub static NSFilenamesPboardType: id;
    pub static NSPasteboardTypePNG: id;
    pub static NSPasteboardTypeString: id;
    pub static NSPasteboardTypeTIFF: id;
}

impl From<ImageFormat> for UTType {
    fn from(value: ImageFormat) -> Self {
        match value {
            ImageFormat::Png => Self::png(),
            ImageFormat::Jpeg => Self::jpeg(),
            ImageFormat::Tiff => Self::tiff(),
            ImageFormat::Webp => Self::webp(),
            ImageFormat::Gif => Self::gif(),
            ImageFormat::Bmp => Self::bmp(),
            ImageFormat::Svg => Self::svg(),
            ImageFormat::Ico => Self::ico(),
            ImageFormat::Pnm => Self::pnm(),
        }
    }
}

// See https://developer.apple.com/documentation/uniformtypeidentifiers/uttype-swift.struct/
pub struct UTType(id);

impl UTType {
    pub fn png() -> Self {
        // https://developer.apple.com/documentation/uniformtypeidentifiers/uttype-swift.struct/png
        Self(unsafe { NSPasteboardTypePNG }) // This is a rare case where there's a built-in NSPasteboardType
    }

    pub fn jpeg() -> Self {
        // https://developer.apple.com/documentation/uniformtypeidentifiers/uttype-swift.struct/jpeg
        Self(unsafe { ns_string("public.jpeg") })
    }

    pub fn gif() -> Self {
        // https://developer.apple.com/documentation/uniformtypeidentifiers/uttype-swift.struct/gif
        Self(unsafe { ns_string("com.compuserve.gif") })
    }

    pub fn webp() -> Self {
        // https://developer.apple.com/documentation/uniformtypeidentifiers/uttype-swift.struct/webp
        Self(unsafe { ns_string("org.webmproject.webp") })
    }

    pub fn bmp() -> Self {
        // https://developer.apple.com/documentation/uniformtypeidentifiers/uttype-swift.struct/bmp
        Self(unsafe { ns_string("com.microsoft.bmp") })
    }

    pub fn svg() -> Self {
        // https://developer.apple.com/documentation/uniformtypeidentifiers/uttype-swift.struct/svg
        Self(unsafe { ns_string("public.svg-image") })
    }

    pub fn ico() -> Self {
        // https://developer.apple.com/documentation/uniformtypeidentifiers/uttype-swift.struct/ico
        Self(unsafe { ns_string("com.microsoft.ico") })
    }

    pub fn tiff() -> Self {
        // https://developer.apple.com/documentation/uniformtypeidentifiers/uttype-swift.struct/tiff
        Self(unsafe { NSPasteboardTypeTIFF }) // This is a rare case where there's a built-in NSPasteboardType
    }

    pub fn pnm() -> Self {
        //https://en.wikipedia.org/w/index.php?title=Netpbm&oldid=1336679433 under Uniform Type Identifier
        Self(unsafe { ns_string("public.pbm") })
    }

    fn inner(&self) -> *const AnyObject {
        self.0
    }

    pub fn inner_mut(&self) -> *mut AnyObject {
        self.0 as *mut _
    }
}
