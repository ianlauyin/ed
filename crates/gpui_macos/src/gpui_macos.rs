#![cfg(target_os = "macos")]
//! macOS platform implementation for GPUI.
//!
//! macOS screens have a y axis that goes up from the bottom of the screen and
//! an origin at the bottom left of the main display.

mod display;
mod display_link;
mod events;
mod keyboard;
mod ns;
mod pasteboard;
mod system_notifications;

#[cfg(feature = "screen-capture")]
mod screen_capture;

use gpui_apple::metal_renderer as renderer;

pub mod metal_renderer {
    pub use gpui_apple::metal_renderer::{PathRasterizationVertex, PathSprite, SurfaceBounds};
}

mod platform;
mod window;
mod window_appearance;

use ns::{NSNotFound, NSUInteger, id};
use objc2::{Encode, Encoding, RefEncode, class, msg_send};
use std::{
    ffi::{CStr, c_char, c_void},
    ops::Range,
};

pub(crate) use display::*;
pub(crate) use display_link::*;
pub(crate) use gpui_apple::{AppleActivity, AppleDispatcher};
pub(crate) use keyboard::*;
pub(crate) use platform::*;
pub(crate) use window::*;

pub use platform::MacPlatform;

trait NSStringExt {
    unsafe fn to_str(&self) -> &str;
}

impl NSStringExt for id {
    unsafe fn to_str(&self) -> &str {
        if self.is_null() {
            return "";
        }
        unsafe {
            let cstr: *const c_char = msg_send![*self, UTF8String];
            if cstr.is_null() {
                ""
            } else {
                CStr::from_ptr(cstr).to_str().unwrap()
            }
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
struct NSRange {
    pub location: NSUInteger,
    pub length: NSUInteger,
}

impl NSRange {
    fn invalid() -> Self {
        Self {
            location: NSNotFound as NSUInteger,
            length: 0,
        }
    }

    fn is_valid(&self) -> bool {
        self.location != NSNotFound as NSUInteger
    }

    fn to_range(self) -> Option<Range<usize>> {
        if self.is_valid() {
            let start = self.location;
            let end = start + self.length;
            Some(start..end)
        } else {
            None
        }
    }
}

impl From<Range<usize>> for NSRange {
    fn from(range: Range<usize>) -> Self {
        NSRange {
            location: range.start as NSUInteger,
            length: range.len() as NSUInteger,
        }
    }
}

// SAFETY: `NSRange` is `#[repr(C)]` with the same layout as Foundation's `_NSRange`.
unsafe impl Encode for NSRange {
    const ENCODING: Encoding = objc2_foundation::NSRange::ENCODING;
}

// SAFETY: see the `Encode` impl above.
unsafe impl RefEncode for NSRange {
    const ENCODING_REF: Encoding = Encoding::Pointer(&Self::ENCODING);
}

/// Returns an autoreleased `NSString`.
unsafe fn ns_string(string: &str) -> id {
    const NS_UTF8_STRING_ENCODING: NSUInteger = 4;
    unsafe {
        let ns_string: id = msg_send![class!(NSString), alloc];
        let ns_string: id = msg_send![
            ns_string,
            initWithBytes: string.as_ptr().cast::<c_void>(),
            length: string.len(),
            encoding: NS_UTF8_STRING_ENCODING
        ];
        msg_send![ns_string, autorelease]
    }
}
