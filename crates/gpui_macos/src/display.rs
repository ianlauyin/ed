use std::time::Duration;

use anyhow::Result;
use gpui::Bounds;
use gpui::DisplayId;
use gpui::Pixels;
use gpui::PlatformDisplay;
use gpui::point;
use gpui::px;
use gpui::refresh_interval_from_hz;
use gpui::size;
use objc2::class;
use objc2::msg_send;
use objc2::sel;
use objc2_core_foundation::CFRetained;
use objc2_core_foundation::CFUUID;
use objc2_core_graphics::CGDirectDisplayID;
use objc2_core_graphics::CGDisplayBounds;
use objc2_core_graphics::CGDisplayCopyDisplayMode;
use objc2_core_graphics::CGDisplayMode;
use objc2_core_graphics::CGError;
use objc2_core_graphics::CGGetActiveDisplayList;
use uuid::Uuid;

use crate::ns::NSRect;
use crate::ns::NSUInteger;
use crate::ns::array_iter;
use crate::ns::array_object_at;
use crate::ns::id;
use crate::ns::nil;
use crate::ns_string;

#[derive(Debug)]
pub(crate) struct MacDisplay(pub(crate) CGDirectDisplayID);

unsafe impl Send for MacDisplay {}

impl MacDisplay {
    /// Get the screen with the given [`DisplayId`].
    pub fn find_by_id(id: DisplayId) -> Option<Self> {
        Self::all().find(|screen| screen.id() == id)
    }

    /// Get the primary screen - the one with the menu bar, and whose bottom left
    /// corner is at the origin of the AppKit coordinate system.
    pub fn primary() -> Self {
        // Instead of iterating through all active systems displays via `all()` we use the first
        // NSScreen and gets its CGDirectDisplayID, because we can't be sure that `CGGetActiveDisplayList`
        // will always return a list of active displays (machine might be sleeping).
        //
        // The following is what Chromium does too:
        //
        // https://chromium.googlesource.com/chromium/src/+/66.0.3359.158/ui/display/mac/screen_mac.mm#56
        unsafe {
            let screens: id = msg_send![class!(NSScreen), screens];
            let screen = array_object_at(screens, 0);
            let device_description: id = msg_send![screen, deviceDescription];
            let screen_number_key: id = ns_string("NSScreenNumber");
            let screen_number: id = msg_send![device_description, objectForKey: screen_number_key];
            let screen_number: NSUInteger = msg_send![screen_number, unsignedIntegerValue];
            Self(screen_number as CGDirectDisplayID)
        }
    }

    /// Obtains an iterator over all currently active system displays.
    ///
    /// Empty if the system can't list them, e.g. without a window server
    /// connection.
    pub fn all() -> impl Iterator<Item = Self> {
        // We're assuming there aren't more than 32 displays connected to the system.
        let mut displays = Vec::with_capacity(32);
        let mut display_count = 0;
        // SAFETY: `displays` has room for the capacity passed in, and
        // `display_count` reports how many entries were written.
        let result = unsafe {
            CGGetActiveDisplayList(displays.capacity() as u32, displays.as_mut_ptr(), &raw mut display_count)
        };
        if result == CGError::Success {
            // SAFETY: `CGGetActiveDisplayList` initialized `display_count`
            // entries, at most the capacity it was given.
            unsafe { displays.set_len(display_count as usize) };
        } else {
            log::error!("Failed to get active display list. Result: {}", result.0);
        }
        displays.into_iter().map(MacDisplay)
    }
}

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn CGDisplayCreateUUIDFromDisplayID(display: CGDirectDisplayID) -> *mut CFUUID;
}

impl PlatformDisplay for MacDisplay {
    fn id(&self) -> DisplayId {
        DisplayId::new(self.0 as u64)
    }

    fn uuid(&self) -> Result<Uuid> {
        let cfuuid = std::ptr::NonNull::new(unsafe { CGDisplayCreateUUIDFromDisplayID(self.0) })
            .ok_or_else(|| anyhow::anyhow!("AppKit returned a null from CGDisplayCreateUUIDFromDisplayID"))?;
        // SAFETY: the function follows the Create rule, so we own this reference.
        let cfuuid = unsafe { CFRetained::from_raw(cfuuid) };
        let bytes = cfuuid.uuid_bytes();
        Ok(Uuid::from_bytes([
            bytes.byte0,
            bytes.byte1,
            bytes.byte2,
            bytes.byte3,
            bytes.byte4,
            bytes.byte5,
            bytes.byte6,
            bytes.byte7,
            bytes.byte8,
            bytes.byte9,
            bytes.byte10,
            bytes.byte11,
            bytes.byte12,
            bytes.byte13,
            bytes.byte14,
            bytes.byte15,
        ]))
    }

    fn bounds(&self) -> Bounds<Pixels> {
        // CGDisplayBounds is in "global display" coordinates, where 0 is
        // the top left of the primary display.
        let bounds = CGDisplayBounds(self.0);

        Bounds {
            origin: gpui::Point::default(),
            size: size(px(bounds.size.width as f32), px(bounds.size.height as f32)),
        }
    }

    fn visible_bounds(&self) -> Bounds<Pixels> {
        unsafe {
            let dominated_screen = self.get_nsscreen();

            if dominated_screen == nil {
                return self.bounds();
            }

            let screen_frame: NSRect = msg_send![dominated_screen, frame];
            let visible_frame: NSRect = msg_send![dominated_screen, visibleFrame];

            // Convert from bottom-left origin (AppKit) to top-left origin
            let origin_y =
                screen_frame.size.height - visible_frame.origin.y - visible_frame.size.height + screen_frame.origin.y;

            Bounds {
                origin: point(px(visible_frame.origin.x as f32 - screen_frame.origin.x as f32), px(origin_y as f32)),
                size: size(px(visible_frame.size.width as f32), px(visible_frame.size.height as f32)),
            }
        }
    }

    fn refresh_interval(&self) -> Option<Duration> {
        // `maximumFramesPerSecond` is the ProMotion maximum, where the
        // display mode's rate is 0 on built-in panels.
        let screen_hertz = unsafe {
            let screen = self.get_nsscreen();
            let supports_maximum: bool =
                if screen == nil { false } else { msg_send![screen, respondsToSelector: sel!(maximumFramesPerSecond)] };
            if supports_maximum {
                let frames_per_second: isize = msg_send![screen, maximumFramesPerSecond];
                frames_per_second as f64
            } else {
                0.0
            }
        };
        refresh_interval_from_hz(screen_hertz).or_else(|| {
            CGDisplayCopyDisplayMode(self.0)
                .and_then(|mode| refresh_interval_from_hz(CGDisplayMode::refresh_rate(Some(&mode))))
        })
    }
}

impl MacDisplay {
    /// Find the NSScreen corresponding to this display
    unsafe fn get_nsscreen(&self) -> id {
        let screens: id = unsafe { msg_send![class!(NSScreen), screens] };
        let screen_number_key: id = unsafe { ns_string("NSScreenNumber") };

        for screen in unsafe { array_iter(screens) } {
            let device_description: id = unsafe { msg_send![screen, deviceDescription] };
            let screen_number: id = unsafe { msg_send![device_description, objectForKey: screen_number_key] };
            let screen_id: NSUInteger = msg_send![screen_number, unsignedIntegerValue];
            if screen_id as CGDirectDisplayID == self.0 {
                return screen;
            }
        }
        nil
    }
}
