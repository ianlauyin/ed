//! Raw Objective-C pointer aliases and small Foundation helpers.
//!
//! Much of this crate still talks to AppKit through `msg_send!` on untyped
//! object pointers; these aliases keep those call sites readable. Prefer the
//! typed `objc2-*` APIs for new code.

#![allow(non_camel_case_types, non_upper_case_globals)]

use std::ptr;

use objc2::class;
use objc2::msg_send;
use objc2::runtime::AnyObject;
pub(crate) use objc2_foundation::NSInteger;
pub(crate) use objc2_foundation::NSPoint;
pub(crate) use objc2_foundation::NSRect;
pub(crate) use objc2_foundation::NSSize;
pub(crate) use objc2_foundation::NSUInteger;

pub(crate) type id = *mut AnyObject;
pub(crate) const nil: id = ptr::null_mut();

pub(crate) const NSNotFound: NSInteger = NSInteger::MAX;

/// Creates a new `NSAutoreleasePool`; release it with `drain`.
pub(crate) unsafe fn autorelease_pool() -> id {
    unsafe { msg_send![class!(NSAutoreleasePool), new] }
}

/// Drains a pool created by [`autorelease_pool`].
pub(crate) unsafe fn drain(pool: id) {
    unsafe {
        let _: () = msg_send![pool, drain];
    }
}

/// Returns an `NSArray`'s element count.
pub(crate) unsafe fn array_count(array: id) -> NSUInteger {
    unsafe { msg_send![array, count] }
}

/// Returns an `NSArray`'s element at `index`.
pub(crate) unsafe fn array_object_at(array: id, index: NSUInteger) -> id {
    unsafe { msg_send![array, objectAtIndex: index] }
}

/// Iterates the elements of an `NSArray` (or other indexed collection).
pub(crate) unsafe fn array_iter(array: id) -> impl Iterator<Item = id> {
    let count = if array.is_null() { 0 } else { unsafe { array_count(array) } };
    (0..count).map(move |index| unsafe { array_object_at(array, index) })
}
