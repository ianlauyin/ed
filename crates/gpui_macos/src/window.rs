use std::cell::Cell;
use std::ffi::CStr;
use std::ffi::CString;
use std::ffi::c_char;
use std::ffi::c_void;
use std::mem;
use std::ops::Range;
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;
use std::ptr::NonNull;
use std::ptr::{self};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::Weak;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

use block2::RcBlock;
use futures::channel::oneshot;
use gpui::AnyWindowHandle;
use gpui::BackgroundExecutor;
use gpui::Bounds;
use gpui::Capslock;
use gpui::CursorStyle;
use gpui::ExternalDragPayload;
use gpui::ExternalPaths;
use gpui::FileDropEvent;
use gpui::ForegroundExecutor;
use gpui::KeyDownEvent;
use gpui::Keystroke;
use gpui::Modifiers;
use gpui::ModifiersChangedEvent;
use gpui::MouseButton;
use gpui::MouseDownEvent;
use gpui::MouseMoveEvent;
use gpui::MouseUpEvent;
use gpui::Pixels;
use gpui::PlatformAtlas;
use gpui::PlatformDisplay;
use gpui::PlatformInput;
use gpui::PlatformInputHandler;
use gpui::PlatformWindow;
use gpui::Point;
use gpui::PromptButton;
use gpui::PromptLevel;
use gpui::RequestFrameOptions;
use gpui::Size;
use gpui::WindowAppearance;
use gpui::WindowBackgroundAppearance;
use gpui::WindowBounds;
use gpui::WindowControlArea;
use gpui::WindowKind;
use gpui::WindowParams;
use gpui::WindowVisibility;
use gpui::point;
use gpui::px;
use gpui::size;
use objc2::AnyThread;
use objc2::ClassType;
use objc2::DefinedClass;
use objc2::MainThreadMarker;
use objc2::MainThreadOnly;
use objc2::class;
use objc2::define_class;
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::AnyClass;
use objc2::runtime::AnyObject;
use objc2::runtime::NSObject;
use objc2::runtime::NSObjectProtocol;
use objc2::runtime::ProtocolObject;
use objc2::runtime::Sel;
use objc2::sel;
use objc2_app_kit::NSAlert;
use objc2_app_kit::NSAlertStyle;
use objc2_app_kit::NSAutoresizingMaskOptions;
use objc2_app_kit::NSBackingStoreType;
use objc2_app_kit::NSBeep;
use objc2_app_kit::NSButton as Objc2NSButton;
use objc2_app_kit::NSDraggingImageComponent;
use objc2_app_kit::NSDraggingImageComponentIconKey;
use objc2_app_kit::NSDraggingItem;
use objc2_app_kit::NSDraggingSource;
use objc2_app_kit::NSEventModifierFlags;
use objc2_app_kit::NSEventType;
use objc2_app_kit::NSPanel;
use objc2_app_kit::NSPasteboardWriting;
use objc2_app_kit::NSRequestUserAttentionType;
use objc2_app_kit::NSResponder;
use objc2_app_kit::NSTextInputClient;
use objc2_app_kit::NSTrackingArea;
use objc2_app_kit::NSTrackingAreaOptions;
use objc2_app_kit::NSView as Objc2NSView;
use objc2_app_kit::NSVisualEffectBlendingMode;
use objc2_app_kit::NSVisualEffectMaterial;
use objc2_app_kit::NSVisualEffectState;
use objc2_app_kit::NSVisualEffectView;
use objc2_app_kit::NSWindow as Objc2NSWindow;
use objc2_app_kit::NSWindowButton as Objc2NSWindowButton;
use objc2_app_kit::NSWindowCollectionBehavior;
use objc2_app_kit::NSWindowOcclusionState;
use objc2_app_kit::NSWindowOrderingMode;
use objc2_app_kit::NSWindowStyleMask;
use objc2_app_kit::NSWindowTitleVisibility;
use objc2_app_kit::NSWorkspace;
use objc2_core_foundation::CFBoolean;
use objc2_core_foundation::CFType;
use objc2_core_graphics::CGDirectDisplayID;
use objc2_foundation::NSCoder;
use objc2_foundation::NSKeyedUnarchiver;
use objc2_foundation::NSRunLoop;
use objc2_foundation::NSRunLoopCommonModes;
use objc2_foundation::NSURL;
use objc2_quartz_core::CADisplayLink;
use objc2_quartz_core::CALayerDelegate;
use parking_lot::Mutex;
use raw_window_handle as rwh;
use smallvec::SmallVec;

use crate::MacDisplay;
use crate::NSRange;
use crate::NSStringExt;
use crate::TISCopyCurrentKeyboardInputSource;
use crate::TISGetInputSourceProperty;
use crate::events::platform_input_from_native;
use crate::kTISPropertyInputSourceIsASCIICapable;
use crate::kTISPropertyInputSourceType;
use crate::kTISTypeKeyboardInputMode;
use crate::ns::NSInteger;
use crate::ns::NSNotFound;
use crate::ns::NSPoint;
use crate::ns::NSRect;
use crate::ns::NSSize;
use crate::ns::NSUInteger;
use crate::ns::array_count;
use crate::ns::array_iter;
use crate::ns::autorelease_pool;
use crate::ns::drain;
use crate::ns::id;
use crate::ns::nil;
use crate::ns_string;
use crate::pasteboard::NSFilenamesPboardType;
use crate::renderer;

/// Borrows a raw `NSWindow` pointer as its typed objc2 counterpart.
unsafe fn ns_window<'a>(window: id) -> &'a Objc2NSWindow {
    unsafe { &*window.cast::<Objc2NSWindow>() }
}

/// Returns an `NSColor` in the sRGB color space.
unsafe fn srgb_color(red: f64, green: f64, blue: f64, alpha: f64) -> id {
    unsafe {
        msg_send![
            class!(NSColor),
            colorWithSRGBRed: red,
            green: green,
            blue: blue,
            alpha: alpha
        ]
    }
}

unsafe fn shared_application() -> id {
    unsafe { msg_send![class!(NSApplication), sharedApplication] }
}

#[allow(non_upper_case_globals)]
const NSWindowStyleMaskNonactivatingPanel: NSWindowStyleMask = NSWindowStyleMask::NonactivatingPanel;
// WindowLevel const value ref: https://docs.rs/core-graphics2/0.4.1/src/core_graphics2/window_level.rs.html
#[allow(non_upper_case_globals)]
const NSNormalWindowLevel: NSInteger = 0;
#[allow(non_upper_case_globals)]
const NSFloatingWindowLevel: NSInteger = 3;
#[allow(non_upper_case_globals)]
const NSPopUpWindowLevel: NSInteger = 101;
#[allow(non_upper_case_globals)]
const NSWindowAnimationBehaviorUtilityWindow: NSInteger = 4;
#[allow(non_upper_case_globals)]
const NSViewLayerContentsRedrawDuringViewResize: NSInteger = 2;
// https://developer.apple.com/documentation/appkit/nsdragoperation
type NSDragOperation = NSUInteger;
#[allow(non_upper_case_globals)]
const NSDragOperationNone: NSDragOperation = 0;
#[allow(non_upper_case_globals)]
const NSDragOperationCopy: NSDragOperation = 1;
#[allow(non_upper_case_globals)]
const NSDragOperationMove: NSDragOperation = 16;
const NSDRAGGING_CONTEXT_OUTSIDE_APPLICATION: NSInteger = 0;
const NSDRAGGING_CONTEXT_WITHIN_APPLICATION: NSInteger = 1;
const NS_WINDOW_TABBING_MODE_DISALLOWED: NSInteger = 2;
/// Instance variables shared by GPUI's window, panel and view classes.
struct WindowStateIvars {
    /// A strong reference to the window's state, installed by `MacWindow::open` right after
    /// the object is initialized and released when the object is deallocated.
    state: Cell<*const Mutex<MacWindowState>>,
}

impl WindowStateIvars {
    const fn new() -> Self {
        Self { state: Cell::new(ptr::null()) }
    }
}

impl Drop for WindowStateIvars {
    fn drop(&mut self) {
        let state = self.state.get();
        if !state.is_null() {
            // SAFETY: `state` was produced by `Arc::into_raw` in `MacWindow::open`.
            drop(unsafe { Arc::from_raw(state) });
        }
    }
}

/// Defines one of GPUI's `NSWindow` subclasses. `GPUIWindow` and `GPUIPanel` share every
/// override; only their AppKit superclass differs.
macro_rules! define_window_class {
    ($class:ident, $name:literal, $($superclass:ident),+) => {
        define_class!(
            // SAFETY: `NSWindow` and `NSPanel` have no subclassing requirements, and the
            // ivars' `Drop` only releases the window state.
            #[unsafe(super($($superclass),+))]
            #[thread_kind = MainThreadOnly]
            #[name = $name]
            #[ivars = WindowStateIvars]
            struct $class;

            impl $class {
                #[unsafe(method(canBecomeMainWindow))]
                fn can_become_main_window(&self) -> bool {
                    true
                }

                #[unsafe(method(canBecomeKeyWindow))]
                fn can_become_key_window(&self) -> bool {
                    true
                }

                #[unsafe(method(windowDidResize:))]
                fn window_did_resize_(&self, notification: id) {
                    window_did_resize(self, notification);
                }

                #[unsafe(method(windowDidChangeOcclusionState:))]
                fn window_did_change_occlusion_state_(&self, notification: id) {
                    window_did_change_occlusion_state(self, notification);
                }

                #[unsafe(method(windowWillEnterFullScreen:))]
                fn window_will_enter_fullscreen_(&self, notification: id) {
                    window_will_enter_fullscreen(self, notification);
                }

                #[unsafe(method(windowWillExitFullScreen:))]
                fn window_will_exit_fullscreen_(&self, notification: id) {
                    window_will_exit_fullscreen(self, notification);
                }

                #[unsafe(method(windowDidFailToExitFullScreen:))]
                fn window_did_fail_to_exit_fullscreen_(&self, notification: id) {
                    window_did_fail_to_exit_fullscreen(self, notification);
                }

                #[unsafe(method(windowDidExitFullScreen:))]
                fn window_did_exit_fullscreen_(&self, notification: id) {
                    window_did_exit_fullscreen(self, notification);
                }

                #[unsafe(method(windowDidMove:))]
                fn window_did_move_(&self, notification: id) {
                    window_did_move(self, notification);
                }

                #[unsafe(method(windowDidChangeScreen:))]
                fn window_did_change_screen_(&self, notification: id) {
                    window_did_change_screen(self, notification);
                }

                #[unsafe(method(windowDidBecomeKey:))]
                fn window_did_become_key(&self, notification: id) {
                    window_did_change_key_status(self, sel!(windowDidBecomeKey:), notification);
                }

                #[unsafe(method(windowDidResignKey:))]
                fn window_did_resign_key(&self, notification: id) {
                    window_did_change_key_status(self, sel!(windowDidResignKey:), notification);
                }

                #[unsafe(method(windowShouldClose:))]
                fn window_should_close_(&self, sender: id) -> bool {
                    window_should_close(self, sender)
                }

                #[unsafe(method(close))]
                fn close_(&self) {
                    close_window(self);
                }

                #[unsafe(method(draggingEntered:))]
                fn dragging_entered_(&self, dragging_info: id) -> NSDragOperation {
                    dragging_entered(self, dragging_info)
                }

                #[unsafe(method(draggingUpdated:))]
                fn dragging_updated_(&self, dragging_info: id) -> NSDragOperation {
                    dragging_updated(self, dragging_info)
                }

                #[unsafe(method(draggingExited:))]
                fn dragging_exited_(&self, dragging_info: id) {
                    dragging_exited(self, dragging_info);
                }

                #[unsafe(method(performDragOperation:))]
                fn perform_drag_operation_(&self, dragging_info: id) -> bool {
                    perform_drag_operation(self, dragging_info)
                }

                #[unsafe(method(concludeDragOperation:))]
                fn conclude_drag_operation_(&self, dragging_info: id) {
                    conclude_drag_operation(self, dragging_info);
                }

                #[unsafe(method(addTitlebarAccessoryViewController:))]
                fn add_titlebar_accessory_view_controller_(&self, view_controller: id) {
                    add_titlebar_accessory_view_controller(self, view_controller);
                }
            }

            unsafe impl NSObjectProtocol for $class {}

            unsafe impl NSDraggingSource for $class {
                #[unsafe(method(draggingSession:sourceOperationMaskForDraggingContext:))]
                fn dragging_session_source_operation_mask_(
                    &self,
                    session: id,
                    context: NSInteger,
                ) -> NSDragOperation {
                    dragging_session_source_operation_mask(self, session, context)
                }

                #[unsafe(method(draggingSession:endedAtPoint:operation:))]
                fn dragging_session_ended_(
                    &self,
                    session: id,
                    screen_point: NSPoint,
                    operation: NSDragOperation,
                ) {
                    dragging_session_ended(self, session, screen_point, operation);
                }
            }
        );
    };
}

define_window_class!(GPUIWindow, "GPUIWindow", Objc2NSWindow, NSResponder, NSObject);
define_window_class!(GPUIPanel, "GPUIPanel", NSPanel, Objc2NSWindow, NSResponder, NSObject);

define_class!(
    // SAFETY: `NSView` has no subclassing requirements, and the ivars' `Drop` only releases the
    // window state.
    #[unsafe(super(Objc2NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "GPUIView"]
    #[ivars = WindowStateIvars]
    struct GPUIView;

    impl GPUIView {
        #[unsafe(method(performKeyEquivalent:))]
        fn perform_key_equivalent(&self, event: id) -> bool {
            handle_key_equivalent(self, event)
        }

        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: id) {
            handle_key_down(self, event);
        }

        #[unsafe(method(keyUp:))]
        fn key_up(&self, event: id) {
            handle_key_up(self, event);
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: id) {
            handle_view_event(self, event);
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: id) {
            handle_view_event(self, event);
        }

        #[unsafe(method(rightMouseDown:))]
        fn right_mouse_down(&self, event: id) {
            handle_view_event(self, event);
        }

        #[unsafe(method(rightMouseUp:))]
        fn right_mouse_up(&self, event: id) {
            handle_view_event(self, event);
        }

        #[unsafe(method(otherMouseDown:))]
        fn other_mouse_down(&self, event: id) {
            handle_view_event(self, event);
        }

        #[unsafe(method(otherMouseUp:))]
        fn other_mouse_up(&self, event: id) {
            handle_view_event(self, event);
        }

        #[unsafe(method(mouseMoved:))]
        fn mouse_moved(&self, event: id) {
            handle_view_event(self, event);
        }

        #[unsafe(method(resetCursorRects))]
        fn reset_cursor_rects_(&self) {
            reset_cursor_rects(self);
        }

        #[unsafe(method(pressureChangeWithEvent:))]
        fn pressure_change_with_event(&self, event: id) {
            handle_view_event(self, event);
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, event: id) {
            handle_view_event(self, event);
        }

        #[unsafe(method(magnifyWithEvent:))]
        fn magnify_with_event(&self, event: id) {
            handle_view_event(self, event);
        }

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: id) {
            handle_view_event(self, event);
        }

        #[unsafe(method(rightMouseDragged:))]
        fn right_mouse_dragged(&self, event: id) {
            handle_view_event(self, event);
        }

        #[unsafe(method(otherMouseDragged:))]
        fn other_mouse_dragged(&self, event: id) {
            handle_view_event(self, event);
        }

        #[unsafe(method(scrollWheel:))]
        fn scroll_wheel(&self, event: id) {
            handle_view_event(self, event);
        }

        #[unsafe(method(swipeWithEvent:))]
        fn swipe_with_event(&self, event: id) {
            handle_view_event(self, event);
        }

        #[unsafe(method(flagsChanged:))]
        fn flags_changed(&self, event: id) {
            handle_view_event(self, event);
        }

        #[unsafe(method(makeBackingLayer))]
        fn make_backing_layer_(&self) -> id {
            make_backing_layer(self)
        }

        #[unsafe(method(viewDidChangeBackingProperties))]
        fn view_did_change_backing_properties_(&self) {
            view_did_change_backing_properties(self);
        }

        #[unsafe(method(setFrameSize:))]
        fn set_frame_size_(&self, size: NSSize) {
            set_frame_size(self, size);
        }

        #[unsafe(method(viewDidChangeEffectiveAppearance))]
        fn view_did_change_effective_appearance_(&self) {
            view_did_change_effective_appearance(self);
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse_(&self, event: id) -> bool {
            accepts_first_mouse(self, event)
        }

        #[unsafe(method(_opaqueRectForWindowMoveWhenInTitlebar))]
        fn opaque_rect_for_window_move_when_in_titlebar_(&self) -> NSRect {
            opaque_rect_for_window_move_when_in_titlebar(self)
        }
    }

    unsafe impl NSObjectProtocol for GPUIView {}

    unsafe impl CALayerDelegate for GPUIView {
        #[unsafe(method(displayLayer:))]
        fn display_layer_(&self, layer: id) {
            display_layer(self, layer);
        }
    }

    impl GPUIView {
        #[unsafe(method(displayLinkDidFire:))]
        fn display_link_did_fire(&self, _: &CADisplayLink) {
            step(self);
        }
    }

    unsafe impl NSTextInputClient for GPUIView {
        #[unsafe(method(validAttributesForMarkedText))]
        fn valid_attributes_for_marked_text_(&self) -> id {
            valid_attributes_for_marked_text(self)
        }

        #[unsafe(method(hasMarkedText))]
        fn has_marked_text_(&self) -> bool {
            has_marked_text(self)
        }

        #[unsafe(method(markedRange))]
        fn marked_range_(&self) -> NSRange {
            marked_range(self)
        }

        #[unsafe(method(selectedRange))]
        fn selected_range_(&self) -> NSRange {
            selected_range(self)
        }

        #[unsafe(method(firstRectForCharacterRange:actualRange:))]
        fn first_rect_for_character_range_(
            &self,
            range: NSRange,
            actual_range: *mut NSRange,
        ) -> NSRect {
            first_rect_for_character_range(self, range, actual_range)
        }

        #[unsafe(method(insertText:replacementRange:))]
        fn insert_text_(&self, text: id, replacement_range: NSRange) {
            insert_text(self, text, replacement_range);
        }

        #[unsafe(method(setMarkedText:selectedRange:replacementRange:))]
        fn set_marked_text_(&self, text: id, selected_range: NSRange, replacement_range: NSRange) {
            set_marked_text(self, text, selected_range, replacement_range);
        }

        #[unsafe(method(unmarkText))]
        fn unmark_text_(&self) {
            unmark_text(self);
        }

        #[unsafe(method(attributedSubstringForProposedRange:actualRange:))]
        fn attributed_substring_for_proposed_range_(
            &self,
            range: NSRange,
            actual_range: *mut NSRange,
        ) -> id {
            attributed_substring_for_proposed_range(self, range, actual_range)
        }

        // Suppress beep on keystrokes with modifier keys.
        #[unsafe(method(doCommandBySelector:))]
        fn do_command_by_selector_(&self, selector: Sel) {
            do_command_by_selector(self, selector);
        }

        #[unsafe(method(characterIndexForPoint:))]
        fn character_index_for_point_(&self, point: NSPoint) -> u64 {
            character_index_for_point(self, point)
        }
    }
);

define_class!(
    // SAFETY: `NSVisualEffectView` has no subclassing requirements, and `BlurredView` has no
    // ivars or `Drop`.
    #[unsafe(super(NSVisualEffectView, Objc2NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "BlurredView"]
    struct BlurredView;

    impl BlurredView {
        #[unsafe(method(initWithFrame:))]
        fn init_with_frame(&self, frame: NSRect) -> id {
            blurred_view_init_with_frame(self, frame)
        }

        #[unsafe(method(updateLayer))]
        fn update_layer(&self) {
            blurred_view_update_layer(self);
        }
    }
);

define_class!(
    // SAFETY: `NSObject` has no subclassing requirements, and the delegate has no ivars or
    // `Drop`.
    #[unsafe(super(NSObject))]
    #[name = "GPUIWindowStateArchiverDelegate"]
    struct GPUIWindowStateArchiverDelegate;

    impl GPUIWindowStateArchiverDelegate {
        #[unsafe(method(archiver:willEncodeObject:))]
        fn archiver_will_encode_object(&self, archiver: id, object: id) -> id {
            window_state_archiver_will_encode_object(self, archiver, object)
        }
    }
);

define_class!(
    // SAFETY: `NSKeyedUnarchiver` has no subclassing requirements, and the subclass has no
    // ivars or `Drop`.
    #[unsafe(super(NSKeyedUnarchiver, NSCoder, NSObject))]
    #[name = "GPUIWindowStateKeyedUnarchiver"]
    struct GPUIWindowStateKeyedUnarchiver;

    impl GPUIWindowStateKeyedUnarchiver {
        #[unsafe(method(_windowRestorationOptions))]
        fn window_restoration_options(&self) -> id {
            window_state_unarchiver_restoration_options(self)
        }
    }
);

// NSKeyedArchiverDelegate callback that skips objects which don't adopt `NSSecureCoding`
// (the window itself and its NSView hierarchy), so encoding the window's restorable state
// succeeds. AppKit still encodes the window frame and its persistent window-management
// identifier, which is what the Space restoration on relaunch keys off.
fn window_state_archiver_will_encode_object(_this: &AnyObject, _archiver: id, object: id) -> id {
    // SAFETY: `object` is whatever AppKit hands the delegate during archiving; we only send it
    // `isKindOfClass:` with valid class arguments, which is safe for any Objective-C object.
    unsafe {
        if object.is_null() {
            return object;
        }
        let is_view: bool = msg_send![object, isKindOfClass: class!(NSView)];
        let is_window: bool = msg_send![object, isKindOfClass: class!(NSWindow)];
        if is_view || is_window { nil } else { object }
    }
}

// Override of the private `_windowRestorationOptions` on our NSKeyedUnarchiver subclass.
// Returning a default-initialized `NSWindowRestorationOptions` tells AppKit to restore the
// window to its original Space (FB15644170: the `NSWindowRestoresWorkspaceAtLaunch`
// user default no longer works).
fn window_state_unarchiver_restoration_options(_this: &AnyObject) -> id {
    // SAFETY: we look the class up by name and only send it `alloc`/`init`/`autorelease`, all of
    // which have the standard `-> id` signature. Returning `nil` when the class is absent is valid.
    unsafe {
        match AnyClass::get(c"NSWindowRestorationOptions") {
            Some(class) => {
                let options: id = msg_send![class, alloc];
                let options: id = msg_send![options, init];
                msg_send![options, autorelease]
            }
            None => nil,
        }
    }
}

pub(crate) fn convert_mouse_position(position: NSPoint, window_height: Pixels) -> Point<Pixels> {
    point(
        px(position.x as f32),
        // macOS screen coordinates are relative to bottom left
        window_height - px(position.y as f32),
    )
}

/// Stores the cursor style on the active GPUI window and invalidates its cursor rects.
///
/// # Safety
///
/// This function is not thread safe. Callers must ensure this is called on the AppKit main
/// thread because it reads the active AppKit window and updates GPUI window state associated
/// with Objective-C objects.
pub(crate) unsafe fn set_active_window_cursor_style(style: CursorStyle) {
    // SAFETY: The caller guarantees AppKit main-thread access. `is_gpui_window` ensures the
    // window carries GPUI window state before reading it.
    unsafe {
        let app = shared_application();
        let key_window: id = msg_send![app, keyWindow];
        let main_window: id = msg_send![app, mainWindow];
        let active_window = if !key_window.is_null() && is_gpui_window(key_window) {
            Some(key_window)
        } else if !main_window.is_null() && is_gpui_window(main_window) {
            Some(main_window)
        } else {
            None
        };

        let Some(active_window) = active_window else {
            return;
        };

        let window_state = get_window_state(&*active_window);
        let mut window_state = window_state.lock();
        if window_state.cursor_style != style {
            window_state.cursor_style = style;
            let _: () = msg_send![
                window_state.native_window,
                invalidateCursorRectsForView: window_state.native_view.as_ptr()
            ];
        }
    }
}

#[derive(Clone, Copy)]
struct TrafficLightFrames {
    titlebar: NSRect,
    close: NSRect,
    minimize: NSRect,
    zoom: NSRect,
}

struct TrafficLightButtons {
    close: Retained<Objc2NSButton>,
    minimize: Retained<Objc2NSButton>,
    zoom: Retained<Objc2NSButton>,
}

// `NSApplicationPresentationOptions` bits (see `NSApplication.PresentationOptions`).
const NS_APPLICATION_PRESENTATION_AUTO_HIDE_DOCK: NSUInteger = 1 << 0;
const NS_APPLICATION_PRESENTATION_AUTO_HIDE_MENU_BAR: NSUInteger = 1 << 2;

// State captured when entering simple (borderless) fullscreen, used to restore
// the window on exit.
struct SimpleFullscreenState {
    frame: NSRect,
    bounds: Bounds<Pixels>,
    style_mask: NSWindowStyleMask,
}

enum SimpleFullscreenPlan {
    Enter { screen_frame: NSRect },
    Exit(SimpleFullscreenState),
}

struct SimpleFullscreenAppState {
    window_count: usize,
    saved_presentation_options: NSUInteger,
}

static SIMPLE_FULLSCREEN_APP_STATE: Mutex<Option<SimpleFullscreenAppState>> = Mutex::new(None);

unsafe fn push_simple_fullscreen_presentation_options() {
    let mut app_state = SIMPLE_FULLSCREEN_APP_STATE.lock();
    match app_state.as_mut() {
        Some(app_state) => app_state.window_count += 1,
        None => unsafe {
            let app = shared_application();
            let saved_presentation_options: NSUInteger = msg_send![app, presentationOptions];
            let _: () = msg_send![
                app,
                setPresentationOptions: NS_APPLICATION_PRESENTATION_AUTO_HIDE_DOCK
                    | NS_APPLICATION_PRESENTATION_AUTO_HIDE_MENU_BAR
            ];
            *app_state = Some(SimpleFullscreenAppState { window_count: 1, saved_presentation_options });
        },
    }
}

unsafe fn pop_simple_fullscreen_presentation_options() {
    let mut app_state = SIMPLE_FULLSCREEN_APP_STATE.lock();
    if let Some(state) = app_state.as_mut() {
        state.window_count = state.window_count.saturating_sub(1);
        if state.window_count == 0 {
            unsafe {
                let app = shared_application();
                let _: () = msg_send![
                    app,
                    setPresentationOptions: state.saved_presentation_options
                ];
            }
            *app_state = None;
        }
    }
}

unsafe fn apply_simple_fullscreen_plan(native_window: id, native_view: id, plan: SimpleFullscreenPlan) {
    unsafe {
        match plan {
            SimpleFullscreenPlan::Exit(saved) => {
                pop_simple_fullscreen_presentation_options();
                let _: () = msg_send![native_window, setStyleMask: saved.style_mask];
                let _: () = msg_send![native_window, setFrame: saved.frame, display: true];
            }
            SimpleFullscreenPlan::Enter { screen_frame } => {
                push_simple_fullscreen_presentation_options();
                let _: () = msg_send![native_window, setStyleMask: NSWindowStyleMask::Borderless];
                let _: () = msg_send![native_window, setFrame: screen_frame, display: true];
            }
        }

        // Changing the style mask makes AppKit resign the window's key status and
        // first responder, so keyboard input stops reaching the editor. Re-make the
        // window key and restore the GPUI view as first responder.
        let _: () = msg_send![native_window, makeKeyAndOrderFront: nil];
        let _: bool = msg_send![native_window, makeFirstResponder: native_view];
    }
}

struct MacWindowState {
    handle: AnyWindowHandle,
    foreground_executor: ForegroundExecutor,
    background_executor: BackgroundExecutor,
    native_window: id,
    native_view: NonNull<AnyObject>,
    blurred_view: Option<id>,
    background_appearance: WindowBackgroundAppearance,
    cursor_style: CursorStyle,
    cursor_visible: Arc<AtomicBool>,
    display_link: Option<Retained<CADisplayLink>>,
    renderer: renderer::Renderer,
    request_frame_callback: Option<Box<dyn FnMut(RequestFrameOptions)>>,
    event_callback: Option<Box<dyn FnMut(PlatformInput) -> gpui::DispatchEventResult>>,
    activate_callback: Option<Box<dyn FnMut(bool)>>,
    visibility_callback: Option<Box<dyn FnMut(WindowVisibility)>>,
    // `None` until a callback is registered, so notifications during
    // construction are not queued for delivery to a callback registered later.
    last_visibility: Option<WindowVisibility>,
    #[allow(clippy::type_complexity)]
    resize_callback: Option<Box<dyn FnMut(Size<Pixels>, f32)>>,
    moved_callback: Option<Box<dyn FnMut()>>,
    display_changed_callback: Option<Box<dyn FnMut()>>,
    should_close_callback: Option<Box<dyn FnMut() -> bool>>,
    close_callback: Option<Box<dyn FnOnce()>>,
    appearance_changed_callback: Option<Box<dyn FnMut()>>,
    input_handler: Option<PlatformInputHandler>,
    last_key_equivalent: Option<KeyDownEvent>,
    last_left_mouse_down_event: Option<Retained<AnyObject>>,
    synthetic_drag_counter: usize,
    traffic_light_position: Option<Point<Pixels>>,
    traffic_light_frames: Option<TrafficLightFrames>,
    pre_fullscreen_traffic_light_frames: Option<TrafficLightFrames>,
    transparent_titlebar: bool,
    previous_modifiers_changed_event: Option<PlatformInput>,
    keystroke_for_do_command: Option<Keystroke>,
    do_command_handled: Option<bool>,
    external_files_dragged: bool,
    // Whether the next left-mouse click is also the focusing click.
    first_mouse: bool,
    // When true, the whole content view is reported as app-owned titlebar content via
    // `_opaqueRectForWindowMoveWhenInTitlebar`, so AppKit does not drag the window from
    // the titlebar or delay titlebar clicks (a delay first observed on macOS 27). Such
    // windows draw their own titlebar and move the window via `start_window_move`.
    app_owns_titlebar_drag: bool,
    fullscreen_restore_bounds: Bounds<Pixels>,
    is_exiting_fullscreen: bool,
    simple_fullscreen_state: Option<SimpleFullscreenState>,
    activated_least_once: bool,
    closed: Arc<AtomicBool>,
    // The parent window if this window is a sheet (Dialog kind)
    sheet_parent: Option<id>,
}

impl MacWindowState {
    fn move_traffic_light(&mut self) {
        if let Some(traffic_light_position) = self.traffic_light_position {
            if self.is_fullscreen() && !self.is_exiting_fullscreen {
                self.restore_traffic_light();
                return;
            }

            if self.traffic_light_frames.is_none() {
                self.traffic_light_frames = self.capture_traffic_light_frames();
            }

            let window_height = Pixels::from(self.native_window().frame().size.height);
            if self.traffic_light_frames.is_some() {
                // AppKit can recreate standard buttons, so fetch the live views for each layout pass.
                let Some(buttons) = self.traffic_light_buttons() else {
                    return;
                };
                let Some(titlebar_container) = Self::titlebar_container(&buttons.close) else {
                    return;
                };

                let close_frame = buttons.close.frame();
                let minimize_frame = buttons.minimize.frame();
                let button_width = Pixels::from(close_frame.size.width);
                let button_height = Pixels::from(close_frame.size.height);
                let button_padding =
                    Pixels::from(minimize_frame.origin.x - close_frame.origin.x - close_frame.size.width);
                let container_height = button_height + traffic_light_position.y + traffic_light_position.y;

                let mut titlebar_frame = titlebar_container.frame();
                titlebar_frame.size.height = container_height.to_f64();
                titlebar_frame.origin.y = (window_height - container_height).to_f64();

                let minimize_x = traffic_light_position.x + button_width + button_padding;
                let zoom_x = minimize_x + button_width + button_padding;

                titlebar_container.setFrame(titlebar_frame);
                buttons
                    .close
                    .setFrameOrigin(NSPoint::new(traffic_light_position.x.to_f64(), traffic_light_position.y.to_f64()));
                buttons.minimize.setFrameOrigin(NSPoint::new(minimize_x.to_f64(), traffic_light_position.y.to_f64()));
                buttons.zoom.setFrameOrigin(NSPoint::new(zoom_x.to_f64(), traffic_light_position.y.to_f64()));

                titlebar_container.updateTrackingAreas();
                buttons.close.updateTrackingAreas();
                buttons.minimize.updateTrackingAreas();
                buttons.zoom.updateTrackingAreas();
            }
        }
    }

    fn capture_traffic_light_frames(&self) -> Option<TrafficLightFrames> {
        let buttons = self.traffic_light_buttons()?;
        let titlebar_container = Self::titlebar_container(&buttons.close)?;

        Some(TrafficLightFrames {
            titlebar: titlebar_container.frame(),
            close: buttons.close.frame(),
            minimize: buttons.minimize.frame(),
            zoom: buttons.zoom.frame(),
        })
    }

    fn native_window(&self) -> &Objc2NSWindow {
        // SAFETY: `MacWindow::new` initializes `self.native_window` with the AppKit
        // window for this state. It is either `NSWindow` or `NSPanel`, so borrowing it
        // as `Objc2NSWindow` is valid here.
        unsafe { &*self.native_window.cast::<Objc2NSWindow>() }
    }

    fn traffic_light_buttons(&self) -> Option<TrafficLightButtons> {
        let window = self.native_window();
        Some(TrafficLightButtons {
            close: window.standardWindowButton(Objc2NSWindowButton::CloseButton)?,
            minimize: window.standardWindowButton(Objc2NSWindowButton::MiniaturizeButton)?,
            zoom: window.standardWindowButton(Objc2NSWindowButton::ZoomButton)?,
        })
    }

    fn titlebar_container(close_button: &Objc2NSButton) -> Option<Retained<Objc2NSView>> {
        // SAFETY: `close_button` comes from AppKit's `standardWindowButton(_:)`.
        // Although `superview` is unsafe, objc2 returns each result as `Retained<NSView>`.
        unsafe {
            let button_container = close_button.superview()?;
            button_container.superview()
        }
    }

    fn restore_traffic_light(&mut self) {
        if let Some(frames) = self.traffic_light_frames.take() {
            let Some(buttons) = self.traffic_light_buttons() else {
                return;
            };
            let Some(titlebar_container) = Self::titlebar_container(&buttons.close) else {
                return;
            };

            buttons.close.setFrame(frames.close);
            buttons.minimize.setFrame(frames.minimize);
            buttons.zoom.setFrame(frames.zoom);
            titlebar_container.setFrame(frames.titlebar);

            titlebar_container.updateTrackingAreas();
            buttons.close.updateTrackingAreas();
            buttons.minimize.updateTrackingAreas();
            buttons.zoom.updateTrackingAreas();
        }
    }

    fn start_display_link(&mut self) {
        unsafe {
            if !ns_window(self.native_window).occlusionState().contains(NSWindowOcclusionState::Visible) {
                self.stop_display_link();
                return;
            }
        }
        // A view's display link follows the view across screens on its own.
        let display_link = self.display_link.get_or_insert_with(|| unsafe {
            let view = &*self.native_view.as_ptr().cast::<Objc2NSView>();
            let display_link = view.displayLinkWithTarget_selector(view, sel!(displayLinkDidFire:));
            // Common modes keep frames coming during live resize and other tracking loops.
            display_link.addToRunLoop_forMode(&NSRunLoop::mainRunLoop(), NSRunLoopCommonModes);
            display_link
        });
        display_link.setPaused(false);
    }

    fn stop_display_link(&self) {
        if let Some(display_link) = self.display_link.as_ref() {
            display_link.setPaused(true);
        }
    }

    fn is_maximized(&self) -> bool {
        fn rect_to_size(rect: NSRect) -> Size<Pixels> {
            let NSSize { width, height } = rect.size;
            size(width.into(), height.into())
        }

        unsafe {
            let bounds = self.bounds();
            let Some(screen) = ns_window(self.native_window).screen() else {
                return false;
            };
            let screen_size = rect_to_size(screen.visibleFrame());
            bounds.size == screen_size
        }
    }

    fn is_fullscreen(&self) -> bool {
        unsafe {
            let style_mask = ns_window(self.native_window).styleMask();
            style_mask.contains(NSWindowStyleMask::FullScreen)
        }
    }

    fn toggle_simple_fullscreen(&mut self) -> Option<SimpleFullscreenPlan> {
        // If the window is in native fullscreen, simple fullscreen would conflict
        // with AppKit's own fullscreen handling, so ignore the request.
        if self.is_fullscreen() {
            return None;
        }

        if let Some(saved) = self.simple_fullscreen_state.take() {
            Some(SimpleFullscreenPlan::Exit(saved))
        } else {
            let window = unsafe { ns_window(self.native_window) };
            let screen = window.screen()?;
            let screen_frame = screen.frame();
            let bounds = self.bounds();

            self.simple_fullscreen_state =
                Some(SimpleFullscreenState { frame: window.frame(), bounds, style_mask: window.styleMask() });

            Some(SimpleFullscreenPlan::Enter { screen_frame })
        }
    }

    fn bounds(&self) -> Bounds<Pixels> {
        let window = unsafe { ns_window(self.native_window) };
        let mut window_frame = window.frame();
        let Some(screen) = window.screen() else {
            return Bounds::new(point(px(0.), px(0.)), gpui::DEFAULT_WINDOW_SIZE);
        };
        let screen_frame = screen.frame();

        // Flip the y coordinate to be top-left origin
        window_frame.origin.y = screen_frame.size.height - window_frame.origin.y - window_frame.size.height;

        Bounds::new(
            point(
                px((window_frame.origin.x - screen_frame.origin.x) as f32),
                px((window_frame.origin.y + screen_frame.origin.y) as f32),
            ),
            size(px(window_frame.size.width as f32), px(window_frame.size.height as f32)),
        )
    }

    fn content_size(&self) -> Size<Pixels> {
        let NSSize { width, height } =
            unsafe { ns_window(self.native_window) }.contentView().map_or(NSSize::default(), |view| view.frame().size);
        size(px(width as f32), px(height as f32))
    }

    fn scale_factor(&self) -> f32 {
        get_scale_factor(self.native_window)
    }

    fn window_bounds(&self) -> WindowBounds {
        if self.is_fullscreen() {
            WindowBounds::Fullscreen(self.fullscreen_restore_bounds)
        } else if let Some(state) = &self.simple_fullscreen_state {
            WindowBounds::Windowed(state.bounds)
        } else {
            WindowBounds::Windowed(self.bounds())
        }
    }
}

unsafe impl Send for MacWindowState {}

pub(crate) struct MacWindow(Arc<Mutex<MacWindowState>>, MainThreadMarker);

impl MacWindow {
    pub fn open(
        handle: AnyWindowHandle,
        WindowParams {
            bounds,
            titlebar,
            kind,
            is_movable,
            app_owns_titlebar_drag,
            is_resizable,
            is_minimizable,
            focus,
            show,
            display_id,
            window_min_size,
            ..
        }: WindowParams,
        cursor_visible: Arc<AtomicBool>,
        foreground_executor: ForegroundExecutor,
        background_executor: BackgroundExecutor,
        renderer_context: renderer::Context,
        marker: MainThreadMarker,
    ) -> Self {
        unsafe {
            let pool = autorelease_pool();

            let mut style_mask;
            if let Some(titlebar) = titlebar.as_ref() {
                style_mask = NSWindowStyleMask::Closable | NSWindowStyleMask::Titled;

                if is_resizable {
                    style_mask |= NSWindowStyleMask::Resizable;
                }

                if is_minimizable {
                    style_mask |= NSWindowStyleMask::Miniaturizable;
                }

                if titlebar.appears_transparent {
                    style_mask |= NSWindowStyleMask::FullSizeContentView;
                }
            } else {
                style_mask = NSWindowStyleMask::Titled | NSWindowStyleMask::FullSizeContentView;
            }

            let use_panel = match kind {
                WindowKind::Normal => false,
                // `AnchoredPopup` is rejected in `MacPlatform::open_window`, grouped here only
                // for exhaustiveness.
                WindowKind::PopUp | WindowKind::AnchoredPopup(_) => {
                    style_mask |= NSWindowStyleMaskNonactivatingPanel;
                    true
                }
                WindowKind::Floating | WindowKind::Dialog => true,
            };

            let display = display_id.and_then(MacDisplay::find_by_id).unwrap_or_else(MacDisplay::primary);

            let mut target_screen = nil;
            let mut screen_frame = None;

            let screens: id = msg_send![class!(NSScreen), screens];
            for screen in array_iter(screens) {
                let Some(display_id) = display_id_for_screen(screen) else {
                    continue;
                };
                let frame: NSRect = msg_send![screen, frame];
                if display_id == display.0 {
                    screen_frame = Some(frame);
                    target_screen = screen;
                }
            }

            let screen_frame = screen_frame.unwrap_or_else(|| {
                let screen: id = msg_send![class!(NSScreen), mainScreen];
                target_screen = screen;
                msg_send![screen, frame]
            });

            let window_rect = NSRect::new(
                NSPoint::new(
                    screen_frame.origin.x + bounds.origin.x.as_f32() as f64,
                    screen_frame.origin.y + (display.bounds().size.height - bounds.origin.y).as_f32() as f64,
                ),
                NSSize::new(bounds.size.width.as_f32() as f64, bounds.size.height.as_f32() as f64),
            );

            // The ivars start out empty; the window state is installed once it exists.
            let native_window: id = if use_panel {
                let panel = GPUIPanel::alloc(marker).set_ivars(WindowStateIvars::new());
                let panel: Retained<GPUIPanel> = msg_send![
                    super(panel),
                    initWithContentRect: window_rect,
                    styleMask: style_mask,
                    backing: NSBackingStoreType::Buffered,
                    defer: false,
                    screen: target_screen
                ];
                Retained::into_raw(panel).cast()
            } else {
                let window = GPUIWindow::alloc(marker).set_ivars(WindowStateIvars::new());
                let window: Retained<GPUIWindow> = msg_send![
                    super(window),
                    initWithContentRect: window_rect,
                    styleMask: style_mask,
                    backing: NSBackingStoreType::Buffered,
                    defer: false,
                    screen: target_screen
                ];
                Retained::into_raw(window).cast()
            };
            let dragged_types: id = msg_send![class!(NSArray), arrayWithObject: NSFilenamesPboardType];
            let () = msg_send![
                native_window,
                registerForDraggedTypes: dragged_types
            ];
            let () = msg_send![
                native_window,
                setReleasedWhenClosed: false
            ];

            let content_view: id = msg_send![native_window, contentView];
            let content_bounds: NSRect = msg_send![content_view, bounds];
            let native_view = GPUIView::alloc(marker).set_ivars(WindowStateIvars::new());
            let native_view: Retained<GPUIView> = msg_send![super(native_view), initWithFrame: content_bounds];
            let native_view: id = Retained::into_raw(native_view).cast();

            let state = Arc::new(Mutex::new(MacWindowState {
                handle,
                foreground_executor,
                background_executor,
                native_window,
                native_view: NonNull::new_unchecked(native_view),
                blurred_view: None,
                background_appearance: WindowBackgroundAppearance::Opaque,
                cursor_style: CursorStyle::Arrow,
                cursor_visible,
                display_link: None,
                renderer: renderer::new_renderer(
                    renderer_context,
                    native_window as *mut _,
                    native_view as *mut _,
                    bounds.size.map(|pixels| pixels.as_f32()),
                    false,
                ),
                request_frame_callback: None,
                event_callback: None,
                activate_callback: None,
                visibility_callback: None,
                last_visibility: None,
                resize_callback: None,
                moved_callback: None,
                display_changed_callback: None,
                should_close_callback: None,
                close_callback: None,
                appearance_changed_callback: None,
                input_handler: None,
                last_key_equivalent: None,
                last_left_mouse_down_event: None,
                synthetic_drag_counter: 0,
                traffic_light_position: titlebar.as_ref().and_then(|titlebar| titlebar.traffic_light_position),
                traffic_light_frames: None,
                pre_fullscreen_traffic_light_frames: None,
                transparent_titlebar: titlebar.as_ref().is_none_or(|titlebar| titlebar.appears_transparent),
                previous_modifiers_changed_event: None,
                keystroke_for_do_command: None,
                do_command_handled: None,
                external_files_dragged: false,
                first_mouse: false,
                app_owns_titlebar_drag,
                fullscreen_restore_bounds: Bounds::default(),
                is_exiting_fullscreen: false,
                simple_fullscreen_state: None,
                activated_least_once: false,
                closed: Arc::new(AtomicBool::new(false)),
                sheet_parent: None,
            }));
            let mut window = Self(state, marker);

            window_state_ivar(&*native_window).set(Arc::into_raw(window.0.clone()));
            let _: () = msg_send![native_window, setDelegate: native_window];
            window_state_ivar(&*native_view).set(Arc::into_raw(window.0.clone()));

            if let Some(title) = titlebar.as_ref().and_then(|t| t.title.as_ref().map(AsRef::as_ref)) {
                window.set_title(title);
            }

            let _: () = msg_send![native_window, setMovable: is_movable];

            if let Some(window_min_size) = window_min_size {
                let _: () = msg_send![native_window, setContentMinSize: NSSize { width: window_min_size.width.to_f64(), height: window_min_size.height.to_f64(), }];
            }

            if titlebar.is_none_or(|titlebar| titlebar.appears_transparent) {
                let _: () = msg_send![native_window, setTitlebarAppearsTransparent: true];
                let _: () = msg_send![native_window, setTitleVisibility: NSWindowTitleVisibility::Hidden];
            }

            let _: () = msg_send![native_view, setAutoresizingMask: NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable];
            let _: () = msg_send![native_view, setWantsBestResolutionOpenGLSurface: true];

            // From winit crate: On Mojave, views automatically become layer-backed shortly after
            // being added to a native_window. Changing the layer-backedness of a view breaks the
            // association between the view and its associated OpenGL context. To work around this,
            // on we explicitly make the view layer-backed up front so that AppKit doesn't do it
            // itself and break the association with its context.
            let _: () = msg_send![native_view, setWantsLayer: true];
            let _: () = msg_send![
            native_view,
            setLayerContentsRedrawPolicy: NSViewLayerContentsRedrawDuringViewResize
            ];

            let native_view_autoreleased: id = msg_send![native_view, autorelease];
            let _: () = msg_send![content_view, addSubview: native_view_autoreleased];
            let _: bool = msg_send![native_window, makeFirstResponder: native_view];

            let app: id = shared_application();
            let main_window: id = msg_send![app, mainWindow];
            let mut sheet_parent = None;

            // SAFETY: This is the live GPUIView (an NSView subclass) added to the
            // content view above, and window creation runs on the main thread.
            let tracking_view = &*native_view.cast::<Objc2NSView>();

            match kind {
                WindowKind::Normal | WindowKind::Floating => {
                    if kind == WindowKind::Floating {
                        // Let the window float keep above normal windows.
                        let _: () = msg_send![native_window, setLevel: NSFloatingWindowLevel];
                        let _: () = msg_send![native_window, setAcceptsMouseMovedEvents: true];
                    } else {
                        let _: () = msg_send![native_window, setLevel: NSNormalWindowLevel];
                        let _: () = msg_send![native_window, setAcceptsMouseMovedEvents: false];
                        add_mouse_tracking_area(tracking_view);
                    }

                    // Every window stands alone: no native window tabs.
                    let _: () = msg_send![native_window, setTabbingMode: NS_WINDOW_TABBING_MODE_DISALLOWED];
                }
                // `AnchoredPopup` is rejected in `MacPlatform::open_window`, grouped here only
                // for exhaustiveness.
                WindowKind::PopUp | WindowKind::AnchoredPopup(_) => {
                    add_mouse_tracking_area(tracking_view);

                    let _: () = msg_send![native_window, setLevel: NSPopUpWindowLevel];
                    let _: () = msg_send![
                        native_window,
                        setAnimationBehavior: NSWindowAnimationBehaviorUtilityWindow
                    ];
                    let _: () = msg_send![native_window, setCollectionBehavior: NSWindowCollectionBehavior::CanJoinAllSpaces | NSWindowCollectionBehavior::FullScreenAuxiliary];
                }
                WindowKind::Dialog => {
                    if !main_window.is_null() {
                        let parent = {
                            let active_sheet: id = msg_send![main_window, attachedSheet];
                            if active_sheet.is_null() { main_window } else { active_sheet }
                        };
                        let _: () = msg_send![parent, beginSheet: native_window, completionHandler: nil];
                        sheet_parent = Some(parent);
                    }
                }
            }

            if focus && show {
                let _: () = msg_send![native_window, makeKeyAndOrderFront: nil];
            } else if show {
                let _: () = msg_send![native_window, orderFront: nil];
            }

            // Set the initial position of the window to the specified origin.
            // Although we already specified the position using `initWithContentRect_styleMask_backing_defer_screen_`,
            // the window position might be incorrect if the main screen (the screen that contains the window that has focus)
            //  is different from the primary screen.
            let _: () = msg_send![native_window, setFrameTopLeftPoint: window_rect.origin];
            {
                let mut window_state = window.0.lock();
                window_state.move_traffic_light();
                window_state.sheet_parent = sheet_parent;
            }

            drain(pool);

            window
        }
    }

    pub fn active_window() -> Option<AnyWindowHandle> {
        unsafe {
            let app = shared_application();
            let main_window: id = msg_send![app, mainWindow];
            if main_window.is_null() {
                return None;
            }

            if msg_send![main_window, isKindOfClass: GPUIWindow::class()] {
                let handle = get_window_state(&*main_window).lock().handle;
                Some(handle)
            } else {
                None
            }
        }
    }

    pub fn ordered_windows() -> Vec<AnyWindowHandle> {
        unsafe {
            let app = shared_application();
            let windows: id = msg_send![app, orderedWindows];
            let count: NSUInteger = msg_send![windows, count];

            let mut window_handles = Vec::new();
            for i in 0..count {
                let window: id = msg_send![windows, objectAtIndex:i];
                if msg_send![window, isKindOfClass: GPUIWindow::class()] {
                    let handle = get_window_state(&*window).lock().handle;
                    window_handles.push(handle);
                }
            }

            window_handles
        }
    }
}

impl Drop for MacWindow {
    fn drop(&mut self) {
        let mut this = self.0.lock();
        this.renderer.destroy();
        let window = this.native_window;
        let sheet_parent = this.sheet_parent.take();
        if let Some(display_link) = this.display_link.take() {
            // The link retains the view as its target until invalidated.
            display_link.invalidate();
        }
        unsafe {
            let _: () = msg_send![this.native_window, setDelegate: nil];
        }
        this.input_handler.take();
        // A delivery task queued by `report_visibility` may still run after the
        // GPUI window is gone; without a callback it has nothing to notify.
        this.visibility_callback.take();
        this.foreground_executor
            .spawn(async move {
                unsafe {
                    if let Some(parent) = sheet_parent {
                        let _: () = msg_send![parent, endSheet: window];
                    }
                    let _: () = msg_send![window, close];
                    let _: id = msg_send![window, autorelease];
                }
            })
            .detach();
    }
}

/// Calls `f` if the window is not closed.
///
/// This should be used when spawning foreground tasks interacting with the
/// window, as some messages will end hard faulting if dispatched to no longer
/// valid window handles.
fn if_window_not_closed(closed: Arc<AtomicBool>, f: impl FnOnce()) {
    if !closed.load(Ordering::Acquire) {
        f();
    }
}

impl PlatformWindow for MacWindow {
    fn bounds(&self) -> Bounds<Pixels> {
        self.0.as_ref().lock().bounds()
    }

    fn window_bounds(&self) -> WindowBounds {
        self.0.as_ref().lock().window_bounds()
    }

    fn is_maximized(&self) -> bool {
        self.0.as_ref().lock().is_maximized()
    }

    fn content_size(&self) -> Size<Pixels> {
        self.0.as_ref().lock().content_size()
    }

    fn resize(&mut self, size: Size<Pixels>) {
        let this = self.0.lock();
        let window = this.native_window;
        let closed = this.closed.clone();
        this.foreground_executor
            .spawn(async move {
                if_window_not_closed(closed, || unsafe {
                    let _: () = msg_send![window, setContentSize: NSSize { width: size.width.as_f32() as f64, height: size.height.as_f32() as f64, }];
                })
            })
            .detach();
    }

    fn native_window_state(&self) -> Option<Vec<u8>> {
        let native_window = {
            let state = self.0.lock();
            if state.is_fullscreen() || state.simple_fullscreen_state.is_some() {
                return None;
            }
            state.native_window
        };
        // SAFETY: `native_window` is a live `NSWindow` retained by this window's state, and the
        // selectors below are AppKit/Foundation methods sent with their documented signatures. The
        // archived bytes are copied into an owned `Vec` before the objects we allocated are
        // released, so no pointer into Objective-C memory escapes this block.
        unsafe {
            let archiver: id = msg_send![class!(NSKeyedArchiver), alloc];
            let archiver: id = msg_send![archiver, initRequiringSecureCoding: true];
            if archiver.is_null() {
                log::warn!("failed to create an archiver for the native window state");
                return None;
            }
            let delegate: id = msg_send![GPUIWindowStateArchiverDelegate::class(), new];
            let _: () = msg_send![archiver, setDelegate: delegate];
            let _: () = msg_send![native_window, encodeRestorableStateWithCoder: archiver];
            let _: () = msg_send![archiver, finishEncoding];
            // The archiver holds a weak reference to its delegate; clear it before the delegate
            // is released below.
            let _: () = msg_send![archiver, setDelegate: nil];

            let data: id = msg_send![archiver, encodedData];
            let bytes: *const c_void = if data.is_null() { ptr::null() } else { msg_send![data, bytes] };
            let state = if bytes.is_null() {
                log::warn!("the archiver produced no data for the native window state");
                None
            } else {
                let length: NSUInteger = msg_send![data, length];
                Some(std::slice::from_raw_parts(bytes.cast::<u8>(), length).to_vec())
            };

            let _: () = msg_send![delegate, release];
            let _: () = msg_send![archiver, release];
            state
        }
    }

    fn restore_native_window_state(&self, state: &[u8]) {
        if state.is_empty() {
            return;
        }
        let native_window = self.0.lock().native_window;
        // SAFETY: `native_window` is a live `NSWindow` retained by this window's state. The NSData,
        // NSKeyedUnarchiver and `restoreStateWithCoder:` selectors are sent with their documented
        // signatures, and the `NSData` only borrows `state` for the duration of this synchronous
        // call (it is consumed before `state` could be freed).
        unsafe {
            let data: id = msg_send![
                class!(NSData),
                dataWithBytes: state.as_ptr() as *const c_void,
                length: state.len()
            ];
            if data.is_null() {
                log::warn!("failed to wrap {} bytes of native window state", state.len());
                return;
            }

            let unarchiver: id = msg_send![GPUIWindowStateKeyedUnarchiver::class(), alloc];
            let mut error: id = nil;
            let unarchiver: id = msg_send![unarchiver, initForReadingFromData: data, error: &mut error];
            if unarchiver.is_null() {
                log::warn!("failed to unarchive the native window state: {}", ns_error_description(error));
                return;
            }
            let _: () = msg_send![native_window, restoreStateWithCoder: unarchiver];
            let error: id = msg_send![unarchiver, error];
            if !error.is_null() {
                log::warn!("failed to restore the native window state: {}", ns_error_description(error));
            }
            let _: () = msg_send![unarchiver, release];
        }
    }

    fn set_traffic_light_position(&self, position: Point<Pixels>) {
        let mut state = self.0.lock();
        state.traffic_light_position = Some(position);
        state.move_traffic_light();
    }

    fn scale_factor(&self) -> f32 {
        self.0.as_ref().lock().scale_factor()
    }

    fn appearance(&self) -> WindowAppearance {
        unsafe {
            let appearance: id = msg_send![self.0.lock().native_window, effectiveAppearance];
            crate::window_appearance::window_appearance_from_native(appearance)
        }
    }

    fn display(&self) -> Option<Rc<dyn PlatformDisplay>> {
        unsafe {
            let screen: id = msg_send![self.0.lock().native_window, screen];
            if screen.is_null() {
                return None;
            }
            let device_description: id = msg_send![screen, deviceDescription];
            let screen_number: id = msg_send![device_description, valueForKey: ns_string("NSScreenNumber")];

            let screen_number: u32 = msg_send![screen_number, unsignedIntValue];

            Some(Rc::new(MacDisplay(screen_number)))
        }
    }

    fn mouse_position(&self) -> Point<Pixels> {
        let position = unsafe {
            let native_window = self.0.lock().native_window;
            msg_send![native_window, mouseLocationOutsideOfEventStream]
        };
        convert_mouse_position(position, self.content_size().height)
    }

    fn modifiers(&self) -> Modifiers {
        unsafe {
            let modifiers: NSEventModifierFlags = msg_send![class!(NSEvent), modifierFlags];

            let control = modifiers.contains(NSEventModifierFlags::Control);
            let alt = modifiers.contains(NSEventModifierFlags::Option);
            let shift = modifiers.contains(NSEventModifierFlags::Shift);
            let command = modifiers.contains(NSEventModifierFlags::Command);
            let function = modifiers.contains(NSEventModifierFlags::Function);

            Modifiers { control, alt, shift, platform: command, function }
        }
    }

    fn capslock(&self) -> Capslock {
        unsafe {
            let modifiers: NSEventModifierFlags = msg_send![class!(NSEvent), modifierFlags];

            Capslock { on: modifiers.contains(NSEventModifierFlags::CapsLock) }
        }
    }

    fn set_input_handler(&mut self, input_handler: PlatformInputHandler) {
        self.0.as_ref().lock().input_handler = Some(input_handler);
    }

    fn take_input_handler(&mut self) -> Option<PlatformInputHandler> {
        self.0.as_ref().lock().input_handler.take()
    }

    fn prompt(
        &self,
        level: PromptLevel,
        msg: &str,
        detail: Option<&str>,
        answers: &[PromptButton],
    ) -> Option<oneshot::Receiver<usize>> {
        use objc2_foundation::NSInteger;
        use objc2_foundation::NSString;

        // NSAlert's first button keeps Return and Cancel keeps Escape, but the keyboard
        // focus (and therefore Space) defaults to Cancel, leaving the middle button of
        // prompts like "Save / Don't Save / Cancel" unreachable from the keyboard. Move
        // the initial focus onto the last non-cancel, non-default button instead.
        let initial_focus_ix = answers
            .iter()
            .enumerate()
            .rev()
            .find(|(_, label)| !label.is_cancel())
            .map(|(ix, _)| ix)
            .filter(|&ix| ix > 0);

        let alert = NSAlert::new(self.1);
        alert.setAlertStyle(match level {
            PromptLevel::Critical => NSAlertStyle::Critical,
            PromptLevel::Warning => NSAlertStyle::Warning,
            PromptLevel::Info => NSAlertStyle::Informational,
        });
        let message = NSString::from_str(msg);
        alert.setMessageText(message.as_ref());

        if let Some(detail) = detail {
            let detail_text = NSString::from_str(detail);
            alert.setInformativeText(detail_text.as_ref());
        }

        let mut initial_focus_button: Option<Retained<Objc2NSButton>> = None;
        for (ix, answer) in answers.iter().enumerate() {
            let title = NSString::from_str(answer.label());
            let button = alert.addButtonWithTitle(&title);
            button.setTag(ix.cast_signed());

            if answer.is_cancel() {
                if let Some(key) = core::char::from_u32(crate::events::ESCAPE_KEY as u32) {
                    let key = NSString::from_str(&key.to_string());
                    button.setKeyEquivalent(&key);
                }
            } else if Some(ix) == initial_focus_ix {
                initial_focus_button = Some(button);
            }
        }

        if let Some(button) = initial_focus_button {
            alert.window().setInitialFirstResponder(Some(&button));
        }

        let (done_tx, done_rx) = oneshot::channel();
        let done_tx = Cell::new(Some(done_tx));

        let block = RcBlock::new(move |answer: NSInteger| {
            if let Some(done_tx) = done_tx.take() {
                let _ = done_tx.send(answer.try_into().unwrap());
            }
        });

        let lock = self.0.lock();
        let native_window = lock.native_window;
        let closed = lock.closed.clone();
        let executor = lock.foreground_executor.clone();
        executor
            .spawn(async move {
                if !closed.load(Ordering::Acquire) {
                    // SAFETY: `native_window` is an Objective-C `NSWindow` pointer
                    // owned by the platform window; bridge it into objc2.
                    let sheet_window: &Objc2NSWindow = unsafe { &*(native_window as *const Objc2NSWindow) };

                    alert.beginSheetModalForWindow_completionHandler(sheet_window, Some(&block));
                }
            })
            .detach();

        Some(done_rx)
    }

    fn activate(&self) {
        let lock = self.0.lock();
        let window = lock.native_window;
        let view = lock.native_view.as_ptr();
        let closed = lock.closed.clone();
        let executor = lock.foreground_executor.clone();
        executor
            .spawn(async move {
                if !closed.load(Ordering::Acquire) {
                    let window = unsafe { &*window.cast::<Objc2NSWindow>() };
                    if !window.isVisible() {
                        let view = unsafe { &*view.cast::<Objc2NSView>() };
                        view.setNeedsDisplay(true);
                    }
                    window.makeKeyAndOrderFront(None);
                }
            })
            .detach();
    }

    fn request_attention(&self) {
        if self.is_active() {
            return;
        }

        let executor = self.0.lock().foreground_executor.clone();
        executor
            .spawn(async move {
                unsafe {
                    let app = shared_application();
                    let _: NSInteger =
                        msg_send![app, requestUserAttention: NSRequestUserAttentionType::InformationalRequest];
                }
            })
            .detach();
    }

    fn is_active(&self) -> bool {
        unsafe { ns_window(self.0.lock().native_window).isKeyWindow() }
    }

    fn visibility(&self) -> WindowVisibility {
        visibility(&self.0.lock())
    }

    // is_hovered is unused on macOS. See Window::is_window_hovered.
    fn is_hovered(&self) -> bool {
        false
    }

    fn set_title(&mut self, title: &str) {
        unsafe {
            let app = shared_application();
            let window = self.0.lock().native_window;
            let title = ns_string(title);
            let _: () = msg_send![app, changeWindowsItem:window, title:title, filename:false];
            let _: () = msg_send![window, setTitle: title];
            self.0.lock().move_traffic_light();
        }
    }

    fn get_title(&self) -> String {
        unsafe {
            let title: id = msg_send![self.0.lock().native_window, title];
            if title.is_null() { "".to_string() } else { title.to_str().to_string() }
        }
    }

    fn set_app_id(&mut self, _app_id: &str) {}

    fn set_background_appearance(&self, background_appearance: WindowBackgroundAppearance) {
        let mut this = self.0.as_ref().lock();
        this.background_appearance = background_appearance;

        let opaque = background_appearance == WindowBackgroundAppearance::Opaque;
        this.renderer.update_transparency(!opaque);

        unsafe {
            let _: () = msg_send![this.native_window, setOpaque: opaque];
            let background_color = if opaque {
                srgb_color(0., 0., 0., 1.)
            } else {
                // Not using `+[NSColor clearColor]` to avoid broken shadow.
                srgb_color(0., 0., 0., 0.0001)
            };
            let _: () = msg_send![this.native_window, setBackgroundColor: background_color];

            if background_appearance != WindowBackgroundAppearance::Blurred {
                if let Some(blur_view) = this.blurred_view {
                    let _: () = msg_send![blur_view, removeFromSuperview];
                    this.blurred_view = None;
                }
            } else if this.blurred_view.is_none() {
                let content_view: id = msg_send![this.native_window, contentView];
                let frame: NSRect = msg_send![content_view, bounds];
                let blur_view: id = msg_send![BlurredView::class(), alloc];
                let blur_view: id = msg_send![blur_view, initWithFrame: frame];
                let _: () = msg_send![blur_view, setAutoresizingMask: NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable];

                let _: () = msg_send![
                    content_view,
                    addSubview: blur_view,
                    positioned: NSWindowOrderingMode::Below,
                    relativeTo: nil
                ];
                let blur_view: id = msg_send![blur_view, autorelease];
                this.blurred_view = Some(blur_view);
            }
        }
    }

    fn background_appearance(&self) -> WindowBackgroundAppearance {
        self.0.as_ref().lock().background_appearance
    }

    fn is_subpixel_rendering_supported(&self) -> bool {
        false
    }

    fn set_edited(&mut self, edited: bool) {
        unsafe {
            let window = self.0.lock().native_window;
            msg_send![window, setDocumentEdited: edited]
        }

        // Changing the document edited state resets the traffic light position,
        // so we have to move it again.
        self.0.lock().move_traffic_light();
    }

    fn set_document_path(&self, path: Option<&std::path::Path>) {
        unsafe {
            let window = self.0.lock().native_window;
            let filename = path.map_or(ns_string(""), |p| ns_string(&p.to_string_lossy()));
            let _: () = msg_send![window, setRepresentedFilename: filename];
        }

        // Changing the document path state resets the traffic light position,
        // so we have to move it again.
        self.0.lock().move_traffic_light();
    }

    fn show_character_palette(&self) {
        let this = self.0.lock();
        let window = this.native_window;
        this.foreground_executor
            .spawn(async move {
                unsafe {
                    let app = shared_application();
                    let _: () = msg_send![app, orderFrontCharacterPalette: window];
                }
            })
            .detach();
    }

    fn minimize(&self) {
        let window = self.0.lock().native_window;
        unsafe {
            let _: () = msg_send![window, miniaturize: nil];
        }
    }

    fn zoom(&self) {
        let this = self.0.lock();
        let window = this.native_window;
        let closed = this.closed.clone();
        this.foreground_executor
            .spawn(async move {
                if_window_not_closed(closed, || unsafe {
                    let _: () = msg_send![window, zoom: nil];
                })
            })
            .detach();
    }

    fn toggle_fullscreen(&self) {
        let this = self.0.lock();
        let window = this.native_window;
        let closed = this.closed.clone();
        this.foreground_executor
            .spawn(async move {
                if_window_not_closed(closed, || unsafe {
                    let _: () = msg_send![window, toggleFullScreen: nil];
                })
            })
            .detach();
    }

    fn toggle_simple_fullscreen(&self) {
        let state = self.0.clone();
        let (foreground_executor, closed) = {
            let this = self.0.lock();
            (this.foreground_executor.clone(), this.closed.clone())
        };
        foreground_executor
            .spawn(async move {
                if_window_not_closed(closed, move || {
                    let (native_window, native_view, plan) = {
                        let mut lock = state.lock();
                        (lock.native_window, lock.native_view.as_ptr() as id, lock.toggle_simple_fullscreen())
                    };
                    if let Some(plan) = plan {
                        unsafe { apply_simple_fullscreen_plan(native_window, native_view, plan) };
                    }
                })
            })
            .detach();
    }

    fn is_simple_fullscreen(&self) -> bool {
        self.0.lock().simple_fullscreen_state.is_some()
    }

    fn is_fullscreen(&self) -> bool {
        let this = self.0.lock();
        let window = this.native_window;

        unsafe { ns_window(window).styleMask().contains(NSWindowStyleMask::FullScreen) }
    }

    fn on_request_frame(&self, callback: Box<dyn FnMut(RequestFrameOptions)>) {
        self.0.as_ref().lock().request_frame_callback = Some(callback);
    }

    fn on_input(&self, callback: Box<dyn FnMut(PlatformInput) -> gpui::DispatchEventResult>) {
        self.0.as_ref().lock().event_callback = Some(callback);
    }

    fn on_active_status_change(&self, callback: Box<dyn FnMut(bool)>) {
        self.0.as_ref().lock().activate_callback = Some(callback);
    }

    fn on_visibility_change(&self, callback: Box<dyn FnMut(WindowVisibility)>) {
        let mut state = self.0.lock();
        state.last_visibility = Some(visibility(&state));
        state.visibility_callback = Some(callback);
    }

    fn on_hover_status_change(&self, _: Box<dyn FnMut(bool)>) {}

    fn on_resize(&self, callback: Box<dyn FnMut(Size<Pixels>, f32)>) {
        self.0.as_ref().lock().resize_callback = Some(callback);
    }

    fn on_display_changed(&self, callback: Box<dyn FnMut()>) {
        self.0.as_ref().lock().display_changed_callback = Some(callback);
    }

    fn on_moved(&self, callback: Box<dyn FnMut()>) {
        self.0.as_ref().lock().moved_callback = Some(callback);
    }

    fn on_should_close(&self, callback: Box<dyn FnMut() -> bool>) {
        self.0.as_ref().lock().should_close_callback = Some(callback);
    }

    fn on_close(&self, callback: Box<dyn FnOnce()>) {
        self.0.as_ref().lock().close_callback = Some(callback);
    }

    fn on_hit_test_window_control(&self, _callback: Box<dyn FnMut() -> Option<WindowControlArea>>) {}

    fn on_appearance_changed(&self, callback: Box<dyn FnMut()>) {
        self.0.lock().appearance_changed_callback = Some(callback);
    }

    fn draw(&self, scene: &gpui::Scene) {
        let mut this = self.0.lock();
        this.renderer.draw(scene);
    }

    fn render_to_image(&self, scene: &gpui::Scene) -> anyhow::Result<image::RgbaImage> {
        self.0.lock().renderer.render_to_image(scene)
    }

    fn sprite_atlas(&self) -> Arc<dyn PlatformAtlas> {
        self.0.lock().renderer.sprite_atlas().clone()
    }

    fn gpu_specs(&self) -> Option<gpui::GpuSpecs> {
        None
    }

    fn update_ime_position(&self, _bounds: Bounds<Pixels>) {
        let executor = self.0.lock().foreground_executor.clone();
        executor
            .spawn(async move {
                unsafe {
                    let input_context: id = msg_send![class!(NSTextInputContext), currentInputContext];
                    if input_context.is_null() {
                        return;
                    }
                    let _: () = msg_send![input_context, invalidateCharacterCoordinates];
                }
            })
            .detach()
    }

    fn titlebar_double_click(&self, is_resizable: bool, is_minimizable: bool) {
        let this = self.0.lock();
        if this.simple_fullscreen_state.is_some() {
            return;
        }
        let window = this.native_window;
        let closed = this.closed.clone();
        this.foreground_executor
            .spawn(async move {
                if_window_not_closed(closed, || {
                    unsafe {
                        let defaults: id = msg_send![class!(NSUserDefaults), standardUserDefaults];
                        let domain = ns_string("NSGlobalDomain");
                        let key = ns_string("AppleActionOnDoubleClick");

                        let dict: id = msg_send![defaults, persistentDomainForName: domain];
                        let action: id = if !dict.is_null() { msg_send![dict, objectForKey: key] } else { nil };

                        let action_str = if !action.is_null() { action.to_str() } else { "" };

                        match action_str {
                            "None" => {
                                // "Do Nothing" selected, so do no action
                            }
                            "Minimize" => {
                                if is_minimizable {
                                    let _: () = msg_send![window, miniaturize: nil];
                                }
                            }
                            "Maximize" => {
                                if is_resizable {
                                    let _: () = msg_send![window, zoom: nil];
                                }
                            }
                            "Fill" => {
                                if is_resizable {
                                    // Unlike `zoom:`, AppKit's private Fill action honors the system's
                                    // "Tiled windows have margins" setting.
                                    let responds_to_zoom_fill: bool =
                                        msg_send![window, respondsToSelector: sel!(_zoomFill:)];
                                    if responds_to_zoom_fill {
                                        let _: () = msg_send![window, _zoomFill: nil];
                                    } else {
                                        let _: () = msg_send![window, zoom: nil];
                                    }
                                }
                            }
                            _ => {
                                if is_resizable {
                                    let _: () = msg_send![window, zoom: nil];
                                }
                            }
                        }
                    }
                })
            })
            .detach();
    }

    fn start_window_move(&self) {
        let this = self.0.lock();
        if this.simple_fullscreen_state.is_some() {
            return;
        }
        let window = this.native_window;

        unsafe {
            let app = shared_application();
            let event: id = msg_send![app, currentEvent];
            let _: () = msg_send![window, performWindowDragWithEvent: event];
        }
    }

    fn can_start_external_drag(&self) -> bool {
        true
    }

    fn start_external_drag(&self, payload: &ExternalDragPayload) -> bool {
        use objc2_foundation::NSArray;
        use objc2_foundation::NSPoint;
        use objc2_foundation::NSRect;
        use objc2_foundation::NSSize;
        use objc2_foundation::NSString;

        let ExternalDragPayload::Files(paths) = payload;
        if paths.entries().is_empty() {
            log::warn!("start_external_drag declined: no paths");
            return false;
        }

        let (native_view, native_window, last_left_mouse_down_event) = {
            let state = self.0.lock();
            (state.native_view.as_ptr(), state.native_window, state.last_left_mouse_down_event.clone())
        };

        let Some(last_left_mouse_down_event) = last_left_mouse_down_event else {
            log::warn!("start_external_drag declined: no retained left mouse down event");
            return false;
        };

        // SAFETY: This method runs on the AppKit/foreground path during drag initiation. The
        // native view/window are retained by MacWindowState, copied out under a short lock above,
        // and all pointers passed to Objective-C remain valid for their respective calls.
        unsafe {
            let event: id = Retained::as_ptr(&last_left_mouse_down_event).cast_mut().cast();
            let dragging_items: id = msg_send![class!(NSMutableArray), array];
            // AppKit keeps this frame's distance from the event's location as the drag image's
            // offset from the cursor, so it has to stay anchored on `event`.
            let location: NSPoint = msg_send![event, locationInWindow];
            let frame = NSRect::new(NSPoint::new(location.x - 16., location.y - 16.), NSSize::new(32., 32.));

            for (path, is_directory) in paths.entries() {
                // Preserve non-UTF-8 paths
                let Ok(path_bytes) = CString::new(path.as_os_str().as_bytes()) else {
                    log::warn!("start_external_drag skipped path containing an interior nul byte");
                    continue;
                };

                let path_bytes = NonNull::new_unchecked(path_bytes.as_ptr().cast_mut());
                let url = NSURL::fileURLWithFileSystemRepresentation_isDirectory_relativeToURL(
                    path_bytes,
                    *is_directory,
                    None,
                );

                let pasteboard_writer = ProtocolObject::<dyn NSPasteboardWriting>::from_ref(&*url);
                let item = NSDraggingItem::initWithPasteboardWriter(NSDraggingItem::alloc(), pasteboard_writer);

                // Resolve drag images lazily via `imageComponentsProvider` (Apple's
                // recommendation for large item counts), and by file *type* rather than
                // `iconForFile:`, which can synchronously hit LaunchServices, network
                // mounts, or iCloud for every selected path and beachball drag startup.
                // `iconForFileType:` is deprecated in favor of `iconForContentType:`,
                // but the replacement needs a `UTType`, which we don't bind.
                let file_type = if *is_directory {
                    "public.folder".to_string()
                } else {
                    path.extension()
                        .and_then(|extension| extension.to_str())
                        .map(|extension| extension.to_string())
                        .unwrap_or_else(|| "public.data".to_string())
                };
                let provider = RcBlock::new(move || {
                    let component =
                        NSDraggingImageComponent::draggingImageComponentWithKey(NSDraggingImageComponentIconKey);
                    let workspace = NSWorkspace::sharedWorkspace();
                    let file_type = NSString::from_str(&file_type);
                    #[expect(deprecated, reason = "`iconForContentType:` needs a `UTType` binding")]
                    let icon = workspace.iconForFileType(&file_type);
                    component.setContents(Some(&icon));
                    // Component frames are relative to the item's dragging frame.
                    component.setFrame(NSRect::new(NSPoint::new(0., 0.), NSSize::new(32., 32.)));
                    let components = NSArray::from_slice(&[&*component]);
                    NonNull::new_unchecked(Retained::autorelease_return(components))
                });
                item.setDraggingFrame(frame);
                item.setImageComponentsProvider(Some(&provider));
                let _: () = msg_send![dragging_items, addObject: Retained::as_ptr(&item)];
            }

            let count: NSUInteger = msg_send![dragging_items, count];
            if count == 0 {
                log::warn!("start_external_drag declined: no dragging items");
                return false;
            }

            let session: id = msg_send![
                native_view,
                beginDraggingSessionWithItems: dragging_items,
                event: event,
                source: native_window
            ];

            let started = !session.is_null();
            if started {
                self.0.lock().synthetic_drag_counter += 1;
            }
            log::debug!("start_external_drag completed: started={}, item_count={}", started, count);
            started
        }
    }

    fn play_system_bell(&self) {
        NSBeep()
    }
}

impl rwh::HasWindowHandle for MacWindow {
    fn window_handle(&self) -> Result<rwh::WindowHandle<'_>, rwh::HandleError> {
        // SAFETY: The AppKitWindowHandle is a wrapper around a pointer to an NSView
        unsafe {
            Ok(rwh::WindowHandle::borrow_raw(rwh::RawWindowHandle::AppKit(rwh::AppKitWindowHandle::new(
                self.0.lock().native_view.cast(),
            ))))
        }
    }
}

impl rwh::HasDisplayHandle for MacWindow {
    fn display_handle(&self) -> Result<rwh::DisplayHandle<'_>, rwh::HandleError> {
        Ok(rwh::DisplayHandle::appkit())
    }
}

fn get_scale_factor(native_window: id) -> f32 {
    let factor = unsafe {
        let screen: id = msg_send![native_window, screen];
        if screen.is_null() {
            return 2.0;
        }
        let factor: f64 = msg_send![screen, backingScaleFactor];
        factor as f32
    };

    // We are not certain what triggers this, but it seems that sometimes
    // this method would return 0 (https://github.com/zed-industries/zed/issues/6412)
    // It seems most likely that this would happen if the window has no screen
    // (if it is off-screen), though we'd expect to see viewDidChangeBackingProperties before
    // it was rendered for real.
    // Regardless, attempt to avoid the issue here.
    if factor == 0.0 { 2. } else { factor }
}

/// Returns whether `window` is one of GPUI's managed windows.
unsafe fn is_gpui_window(window: id) -> bool {
    unsafe {
        msg_send![window, isKindOfClass: GPUIWindow::class()] || msg_send![window, isKindOfClass: GPUIPanel::class()]
    }
}

/// Returns the window-state ivar of a `GPUIView`, `GPUIWindow` or `GPUIPanel`.
fn window_state_ivar(object: &AnyObject) -> &Cell<*const Mutex<MacWindowState>> {
    if let Some(view) = object.downcast_ref::<GPUIView>() {
        &view.ivars().state
    } else if let Some(window) = object.downcast_ref::<GPUIWindow>() {
        &window.ivars().state
    } else if let Some(panel) = object.downcast_ref::<GPUIPanel>() {
        &panel.ivars().state
    } else {
        panic!("{object:?} is not a GPUI window or view")
    }
}

unsafe fn get_window_state(object: &AnyObject) -> Arc<Mutex<MacWindowState>> {
    unsafe {
        let raw = window_state_ivar(object).get();
        let rc1 = Arc::from_raw(raw);
        let rc2 = rc1.clone();
        mem::forget(rc1);
        rc2
    }
}

fn add_mouse_tracking_area(native_view: &Objc2NSView) {
    let options = NSTrackingAreaOptions::MouseEnteredAndExited
        | NSTrackingAreaOptions::MouseMoved
        // Track even when another application is active so visible
        // windows can respond to hover without being focused.
        | NSTrackingAreaOptions::ActiveAlways
        | NSTrackingAreaOptions::InVisibleRect;

    // SAFETY: NSView provides the tracking-event callbacks, and the owner is
    // the same view that retains the tracking area. No user info is supplied.
    let tracking_area = unsafe {
        NSTrackingArea::initWithRect_options_owner_userInfo(
            NSTrackingArea::alloc(),
            NSRect::ZERO,
            options,
            Some(native_view),
            None,
        )
    };
    native_view.addTrackingArea(&tracking_area);
}

fn reset_cursor_rects(this: &AnyObject) {
    // SAFETY: AppKit invokes cursor-rect updates on the main thread for GPUIView instances,
    // whose window-state ivar is initialized when the view is created. The cursor registered
    // below is a valid NSCursor.
    unsafe {
        let _: () = msg_send![super(this, class!(NSView)), resetCursorRects];

        let window_state = get_window_state(this);
        let cursor_style = window_state.lock().cursor_style;

        let cursor: id = match cursor_style {
            CursorStyle::Arrow => msg_send![class!(NSCursor), arrowCursor],
            CursorStyle::IBeam => msg_send![class!(NSCursor), IBeamCursor],
            CursorStyle::Crosshair => msg_send![class!(NSCursor), crosshairCursor],
            CursorStyle::ClosedHand => msg_send![class!(NSCursor), closedHandCursor],
            CursorStyle::OpenHand => msg_send![class!(NSCursor), openHandCursor],
            CursorStyle::PointingHand => msg_send![class!(NSCursor), pointingHandCursor],
            CursorStyle::ResizeLeftRight => msg_send![class!(NSCursor), resizeLeftRightCursor],
            CursorStyle::ResizeUpDown => msg_send![class!(NSCursor), resizeUpDownCursor],
            CursorStyle::ResizeLeft => msg_send![class!(NSCursor), resizeLeftCursor],
            CursorStyle::ResizeRight => msg_send![class!(NSCursor), resizeRightCursor],
            CursorStyle::ResizeColumn => msg_send![class!(NSCursor), resizeLeftRightCursor],
            CursorStyle::ResizeRow => msg_send![class!(NSCursor), resizeUpDownCursor],
            CursorStyle::ResizeUp => msg_send![class!(NSCursor), resizeUpCursor],
            CursorStyle::ResizeDown => msg_send![class!(NSCursor), resizeDownCursor],

            // Undocumented, private class methods:
            // https://stackoverflow.com/questions/27242353/cocoa-predefined-resize-mouse-cursor
            CursorStyle::ResizeUpLeftDownRight => {
                msg_send![class!(NSCursor), _windowResizeNorthWestSouthEastCursor]
            }
            CursorStyle::ResizeUpRightDownLeft => {
                msg_send![class!(NSCursor), _windowResizeNorthEastSouthWestCursor]
            }

            CursorStyle::IBeamCursorForVerticalLayout => {
                msg_send![class!(NSCursor), IBeamCursorForVerticalLayout]
            }
            CursorStyle::OperationNotAllowed => {
                msg_send![class!(NSCursor), operationNotAllowedCursor]
            }
            CursorStyle::DragLink => msg_send![class!(NSCursor), dragLinkCursor],
            CursorStyle::DragCopy => msg_send![class!(NSCursor), dragCopyCursor],
            CursorStyle::ContextualMenu => msg_send![class!(NSCursor), contextualMenuCursor],
        };

        let bounds: NSRect = msg_send![this, bounds];
        let _: () = msg_send![this, addCursorRect: bounds, cursor: cursor];
    }
}

fn handle_key_equivalent(this: &AnyObject, native_event: id) -> bool {
    handle_key_event(this, native_event, true)
}

fn handle_key_down(this: &AnyObject, native_event: id) {
    handle_key_event(this, native_event, false);
}

fn handle_key_up(this: &AnyObject, native_event: id) {
    handle_key_event(this, native_event, false);
}

// Things to test if you're modifying this method:
//  U.S. layout:
//   - The IME consumes characters like 'j' and 'k', which makes paging through `less` in
//     the terminal behave incorrectly by default. This behavior should be patched by our
//     IME integration
//   - `alt-t` should open the tasks menu
//   - In vim mode, this keybinding should work:
//     ```
//        {
//          "context": "Editor && vim_mode == insert",
//          "bindings": {"j j": "vim::NormalBefore"}
//        }
//     ```
//     and typing 'j k' in insert mode with this keybinding should insert the two characters
//  Brazilian layout:
//   - `" space` should create an unmarked quote
//   - `" backspace` should delete the marked quote
//   - `" "`should create an unmarked quote and a second marked quote
//   - `" up` should insert a quote, unmark it, and move up one line
//   - `" cmd-down` should insert a quote, unmark it, and move to the end of the file
//   - `cmd-ctrl-space` and clicking on an emoji should type it
//  Czech (QWERTY) layout:
//   - in vim mode `option-4`  should go to end of line (same as $)
//  Japanese (Romaji) layout:
//   - type `a i left down up enter enter` should create an unmarked text "愛"
//   - In vim mode with `jj` bound to `vim::NormalBefore` in insert mode, typing 'j i' with
//     Japanese IME should produce "じ" (ji), not "jい"

/// Returns true if the current keyboard input source is a composition-based IME
/// (e.g. Japanese Hiragana, Korean, Chinese Pinyin) that produces non-ASCII output.
///
/// This checks two properties:
/// 1. The source type is `kTISTypeKeyboardInputMode` (an IME input mode, not a plain
///    keyboard layout). This excludes non-ASCII layouts like Armenian and Ukrainian
///    that map keys directly without composition.
/// 2. The source is not ASCII-capable, which excludes modes like Japanese Romaji that
///    produce ASCII characters and should allow multi-stroke keybindings like `jj`.
unsafe fn is_ime_input_source_active() -> bool {
    unsafe {
        let source = TISCopyCurrentKeyboardInputSource();
        if source.is_null() {
            return false;
        }

        let source_type = TISGetInputSourceProperty(source, kTISPropertyInputSourceType as *const c_void);
        let is_input_mode =
            !source_type.is_null() && *source_type.cast::<CFType>() == *kTISTypeKeyboardInputMode.cast::<CFType>();

        let is_ascii = TISGetInputSourceProperty(source, kTISPropertyInputSourceIsASCIICapable as *const c_void);
        let is_ascii_capable = !is_ascii.is_null() && (*is_ascii.cast::<CFBoolean>()).value();

        let _: () = msg_send![source, release];

        is_input_mode && !is_ascii_capable
    }
}

fn handle_key_event(this: &AnyObject, native_event: id, key_equivalent: bool) -> bool {
    let window_state = unsafe { get_window_state(this) };
    let mut lock = window_state.as_ref().lock();

    let window_height = lock.content_size().height;
    let event = unsafe { platform_input_from_native(native_event, Some(window_height)) };

    let Some(event) = event else {
        return false;
    };

    let run_callback = |event: PlatformInput| -> bool {
        let mut callback = window_state.as_ref().lock().event_callback.take();
        let handled: bool = if let Some(callback) = callback.as_mut() { !callback(event).propagate } else { false };
        window_state.as_ref().lock().event_callback = callback;
        handled
    };

    match event {
        PlatformInput::KeyDown(key_down_event) => {
            // For certain keystrokes, macOS will first dispatch a "key equivalent" event.
            // If that event isn't handled, it will then dispatch a "key down" event. GPUI
            // makes no distinction between these two types of events, so we need to ignore
            // the "key down" event if we've already just processed its "key equivalent" version.
            if key_equivalent {
                lock.last_key_equivalent = Some(key_down_event.clone());
            } else if lock.last_key_equivalent.take().as_ref() == Some(&key_down_event) {
                return false;
            }

            drop(lock);

            let is_composing =
                with_input_handler(this, |input_handler| input_handler.marked_text_range()).flatten().is_some();

            // If we're composing, send the key to the input handler first;
            // otherwise we only send to the input handler if we don't have a matching binding.
            // The input handler may call `do_command_by_selector` if it doesn't know how to handle
            // a key. If it does so, it will return true so we won't send the key twice.
            // We also do this for non-printing keys (like arrow keys and escape) as the IME menu
            // may need them even if there is no marked text;
            // however we skip keys with control or the input handler adds control-characters to the buffer.
            // and keys with function, as the input handler swallows them.
            // and keys with platform (Cmd), so that Cmd+key events (e.g. Cmd+`) are not
            // consumed by the IME on non-QWERTY / dead-key layouts.
            // We also send printable keys to the IME first when an IME input source (e.g. Japanese,
            // Korean, Chinese) is active and the input handler accepts text input. This prevents
            // multi-stroke keybindings like `jj` from intercepting keys that the IME should compose
            // (e.g. typing 'ji' should produce 'じ', not 'jい'). If the IME doesn't handle the key,
            // it calls `doCommandBySelector:` which routes it back to keybinding matching.
            let is_ime_printable_key = !is_composing
                && key_down_event
                    .keystroke
                    .key_char
                    .as_ref()
                    .is_some_and(|key_char| key_char.chars().all(|c| !c.is_control()))
                && !key_down_event.keystroke.modifiers.control
                && !key_down_event.keystroke.modifiers.function
                && !key_down_event.keystroke.modifiers.platform
                && unsafe { is_ime_input_source_active() }
                && with_input_handler(this, |input_handler| input_handler.query_prefers_ime_for_printable_keys())
                    .unwrap_or(false);

            if is_composing
                || is_ime_printable_key
                || (key_down_event.keystroke.key_char.is_none()
                    && !key_down_event.keystroke.modifiers.control
                    && !key_down_event.keystroke.modifiers.function
                    && !key_down_event.keystroke.modifiers.platform)
            {
                {
                    let mut lock = window_state.as_ref().lock();
                    lock.keystroke_for_do_command = Some(key_down_event.keystroke.clone());
                    lock.do_command_handled.take();
                    drop(lock);
                }

                let handled = unsafe { input_context_handle_event(this, native_event) };
                window_state.as_ref().lock().keystroke_for_do_command.take();
                if let Some(handled) = window_state.as_ref().lock().do_command_handled.take() {
                    return handled;
                } else if handled {
                    return true;
                }

                let handled = run_callback(PlatformInput::KeyDown(key_down_event));
                return handled;
            }

            let handled = run_callback(PlatformInput::KeyDown(key_down_event.clone()));
            if handled {
                return true;
            }

            if key_down_event.is_held
                && let Some(key_char) = key_down_event.keystroke.key_char.as_ref()
            {
                let handled = with_input_handler(this, |input_handler| {
                    if !input_handler.apple_press_and_hold_enabled() {
                        input_handler.replace_text_in_range(None, key_char);
                        return true;
                    }
                    false
                });
                if handled == Some(true) {
                    return true;
                }
            }

            // Don't send key equivalents to the input handler if there are key modifiers other
            // than Function key, or macOS shortcuts like cmd-` will stop working.
            if key_equivalent && key_down_event.keystroke.modifiers != Modifiers::function() {
                return false;
            }

            unsafe { input_context_handle_event(this, native_event) }
        }

        PlatformInput::KeyUp(_) => {
            drop(lock);
            run_callback(event)
        }

        _ => false,
    }
}

fn handle_view_event(this: &AnyObject, native_event: id) {
    let window_state = unsafe { get_window_state(this) };
    let weak_window_state = Arc::downgrade(&window_state);
    let mut lock = window_state.as_ref().lock();
    let window_height = lock.content_size().height;
    let native_event_type: NSEventType = unsafe { msg_send![native_event, type] };
    match native_event_type {
        NSEventType::LeftMouseDown => {
            // AppKit owns `native_event` for the callback; retain it so the drag session can still
            // be started later, once the pointer leaves the window.
            lock.last_left_mouse_down_event = unsafe { Retained::retain(native_event.cast::<AnyObject>()) };
        }
        NSEventType::LeftMouseUp => {
            lock.last_left_mouse_down_event = None;
        }
        _ => {}
    }
    let event = unsafe { platform_input_from_native(native_event, Some(window_height)) };

    if let Some(mut event) = event {
        // AppKit unhides the cursor on the next mouse movement; mirror that here.
        if matches!(
            event,
            PlatformInput::MouseMove(_)
                | PlatformInput::MouseDown(_)
                | PlatformInput::MouseUp(_)
                | PlatformInput::MousePressure(_)
                | PlatformInput::MouseExited(_)
                | PlatformInput::ScrollWheel(_)
                | PlatformInput::Pinch(_)
        ) {
            lock.cursor_visible.store(true, Ordering::Relaxed);
        }

        match &mut event {
            PlatformInput::MouseDown(
                event @ MouseDownEvent { button: MouseButton::Left, modifiers: Modifiers { control: true, .. }, .. },
            ) => {
                // On mac, a ctrl-left click should be handled as a right click.
                *event = MouseDownEvent {
                    button: MouseButton::Right,
                    modifiers: Modifiers { control: false, ..event.modifiers },
                    click_count: 1,
                    ..*event
                };
            }

            // Handles focusing click.
            PlatformInput::MouseDown(event @ MouseDownEvent { button: MouseButton::Left, .. })
                if (lock.first_mouse) =>
            {
                *event = MouseDownEvent { first_mouse: true, ..*event };
                lock.first_mouse = false;
            }

            // Because we map a ctrl-left_down to a right_down -> right_up let's ignore
            // the ctrl-left_up to avoid having a mismatch in button down/up events if the
            // user is still holding ctrl when releasing the left mouse button
            PlatformInput::MouseUp(
                event @ MouseUpEvent { button: MouseButton::Left, modifiers: Modifiers { control: true, .. }, .. },
            ) => {
                *event = MouseUpEvent {
                    button: MouseButton::Right,
                    modifiers: Modifiers { control: false, ..event.modifiers },
                    click_count: 1,
                    ..*event
                };
            }

            _ => {}
        };

        match &event {
            PlatformInput::MouseDown(_) => {
                drop(lock);
                unsafe { input_context_handle_event(this, native_event) };
                lock = window_state.as_ref().lock();
            }
            PlatformInput::MouseMove(event @ MouseMoveEvent { pressed_button: Some(_), .. }) => {
                // Synthetic drag is used for selecting long buffer contents while buffer is being scrolled.
                // External file drag and drop is able to emit its own synthetic mouse events which will conflict
                // with these ones.
                if !lock.external_files_dragged {
                    lock.synthetic_drag_counter += 1;
                    let executor = lock.foreground_executor.clone();
                    executor
                        .spawn(synthetic_drag(
                            weak_window_state,
                            lock.synthetic_drag_counter,
                            event.clone(),
                            lock.background_executor.clone(),
                        ))
                        .detach();
                }
            }

            PlatformInput::MouseUp(MouseUpEvent { .. }) => {
                lock.synthetic_drag_counter += 1;
            }

            PlatformInput::ModifiersChanged(ModifiersChangedEvent { modifiers, capslock }) => {
                // Only raise modifiers changed event when they have actually changed
                if let Some(PlatformInput::ModifiersChanged(ModifiersChangedEvent {
                    modifiers: prev_modifiers,
                    capslock: prev_capslock,
                })) = &lock.previous_modifiers_changed_event
                    && prev_modifiers == modifiers
                    && prev_capslock == capslock
                {
                    return;
                }

                lock.previous_modifiers_changed_event = Some(event.clone());
            }

            _ => {}
        }

        if let Some(mut callback) = lock.event_callback.take() {
            drop(lock);
            callback(event);
            window_state.lock().event_callback = Some(callback);
        }
    }
}

fn visibility(state: &MacWindowState) -> WindowVisibility {
    let is_visible =
        unsafe { ns_window(state.native_window).occlusionState().contains(NSWindowOcclusionState::Visible) };
    if is_visible { WindowVisibility::Visible } else { WindowVisibility::Hidden }
}

fn report_visibility(window_state: &Arc<Mutex<MacWindowState>>) {
    let state = window_state.lock();
    if state.last_visibility.is_none() {
        return;
    }
    let executor = state.foreground_executor.clone();
    drop(state);

    // AppKit can notify while GPUI is updating a window. Deliver observers
    // after that update completes, as activation notifications do. The state
    // is read at delivery rather than captured here so a burst of
    // notifications collapses to the final value.
    executor
        .spawn({
            let window_state = window_state.clone();
            async move {
                let mut state = window_state.lock();
                let visibility = visibility(&state);
                if state.last_visibility == Some(visibility) {
                    return;
                }
                state.last_visibility = Some(visibility);
                if let Some(mut callback) = state.visibility_callback.take() {
                    drop(state);
                    callback(visibility);
                    window_state.lock().visibility_callback = Some(callback);
                }
            }
        })
        .detach();
}

fn window_did_change_occlusion_state(this: &AnyObject, _: id) {
    let window_state = unsafe { get_window_state(this) };
    let mut lock = window_state.lock();
    unsafe {
        if ns_window(lock.native_window).occlusionState().contains(NSWindowOcclusionState::Visible) {
            lock.move_traffic_light();
            lock.start_display_link();
        } else {
            lock.stop_display_link();
        }
    }
    drop(lock);
    // The only visibility source: AppKit posts this for covering, minimizing,
    // hiding, Space switches, and display sleep alike.
    report_visibility(&window_state);
}

fn window_did_resize(this: &AnyObject, _: id) {
    let window_state = unsafe { get_window_state(this) };
    window_state.as_ref().lock().move_traffic_light();
}

fn window_will_enter_fullscreen(this: &AnyObject, _: id) {
    let window_state = unsafe { get_window_state(this) };
    let mut lock = window_state.as_ref().lock();
    lock.fullscreen_restore_bounds = lock.bounds();
    lock.is_exiting_fullscreen = false;
    lock.pre_fullscreen_traffic_light_frames = lock.traffic_light_frames;
    lock.restore_traffic_light();

    unsafe {
        let _: () = msg_send![lock.native_window, setTitlebarAppearsTransparent: false];
    }
}

fn window_will_exit_fullscreen(this: &AnyObject, _: id) {
    let window_state = unsafe { get_window_state(this) };
    let mut lock = window_state.as_ref().lock();
    lock.is_exiting_fullscreen = true;

    if lock.transparent_titlebar {
        unsafe {
            let _: () = msg_send![lock.native_window, setTitlebarAppearsTransparent: true];
        }
    }

    lock.move_traffic_light();
}

fn window_did_fail_to_exit_fullscreen(this: &AnyObject, _: id) {
    let window_state = unsafe { get_window_state(this) };
    let mut lock = window_state.as_ref().lock();
    lock.is_exiting_fullscreen = false;
    lock.restore_traffic_light();
}

fn window_did_exit_fullscreen(this: &AnyObject, _: id) {
    // SAFETY: This method is registered only on GPUI window classes, which initialize
    // window-state ivar with an Arc<Mutex<MacWindowState>> during window creation.
    let window_state = unsafe { get_window_state(this) };
    let mut lock = window_state.as_ref().lock();
    lock.is_exiting_fullscreen = false;
    // Moving the buttons during the transition captures their fullscreen frames.
    // Keep using the native windowed frames when restoring them on the next entry.
    lock.traffic_light_frames = lock.pre_fullscreen_traffic_light_frames.take();
    lock.move_traffic_light();
}

/// Forwards `native_event` to the view's input context, returning whether it was handled.
unsafe fn input_context_handle_event(view: &AnyObject, native_event: id) -> bool {
    unsafe {
        let input_context: id = msg_send![view, inputContext];
        !input_context.is_null() && msg_send![input_context, handleEvent: native_event]
    }
}

fn ns_error_description(error: id) -> String {
    if error.is_null() {
        return "unknown error".to_owned();
    }
    unsafe {
        let description: id = msg_send![error, localizedDescription];
        description.to_str().to_owned()
    }
}

fn window_did_move(this: &AnyObject, _: id) {
    let window_state = unsafe { get_window_state(this) };
    let mut lock = window_state.as_ref().lock();
    if let Some(mut callback) = lock.moved_callback.take() {
        drop(lock);
        callback();
        window_state.lock().moved_callback = Some(callback);
    }
}

// Update the window scale factor and drawable size, and call the resize callback if any.
fn update_window_scale_factor(window_state: &Arc<Mutex<MacWindowState>>) {
    let mut lock = window_state.as_ref().lock();
    let scale_factor = lock.scale_factor();
    let size = lock.content_size();
    let drawable_size = size.to_device_pixels(scale_factor);
    if let Some(layer) = lock.renderer.layer() {
        layer.setContentsScale(scale_factor as f64);
    }

    lock.renderer.update_drawable_size(drawable_size);

    if let Some(mut callback) = lock.resize_callback.take() {
        let content_size = lock.content_size();
        let scale_factor = lock.scale_factor();
        drop(lock);
        callback(content_size, scale_factor);
        window_state.as_ref().lock().resize_callback = Some(callback);
    };
}

fn window_did_change_screen(this: &AnyObject, _: id) {
    let window_state = unsafe { get_window_state(this) };
    let mut lock = window_state.as_ref().lock();
    lock.start_display_link();
    drop(lock);
    update_window_scale_factor(&window_state);
    report_display_change(&window_state);
}

fn report_display_change(window_state: &Arc<Mutex<MacWindowState>>) {
    let executor = window_state.lock().foreground_executor.clone();
    // AppKit can post screen changes while GPUI is updating a window, e.g.
    // from `setFrame:`, so deliver after that update completes.
    executor
        .spawn({
            let window_state = window_state.clone();
            async move {
                let callback = window_state.lock().display_changed_callback.take();
                if let Some(mut callback) = callback {
                    callback();
                    window_state.lock().display_changed_callback = Some(callback);
                }
            }
        })
        .detach();
}

fn window_did_change_key_status(this: &AnyObject, selector: Sel, _: id) {
    let window_state = unsafe { get_window_state(this) };
    let lock = window_state.lock();
    let is_active = unsafe { ns_window(lock.native_window).isKeyWindow() };

    // AppKit also unhides the cursor on activation changes, so mirror that here.
    lock.cursor_visible.store(true, Ordering::Relaxed);

    // When opening a pop-up while the application isn't active, Cocoa sends a spurious
    // `windowDidBecomeKey` message to the previous key window even though that window
    // isn't actually key. This causes a bug if the application is later activated while
    // the pop-up is still open, making it impossible to activate the previous key window
    // even if the pop-up gets closed. The only way to activate it again is to de-activate
    // the app and re-activate it, which is a pretty bad UX.
    // The following code detects the spurious event and invokes `resignKeyWindow`:
    // in theory, we're not supposed to invoke this method manually but it balances out
    // the spurious `becomeKeyWindow` event and helps us work around that bug.
    if selector == sel!(windowDidBecomeKey:) && !is_active {
        let native_window = lock.native_window;
        drop(lock);
        unsafe {
            let _: () = msg_send![native_window, resignKeyWindow];
        }
        return;
    }

    let executor = lock.foreground_executor.clone();
    drop(lock);

    // When a window becomes active, trigger an immediate synchronous frame request to prevent
    // tab flicker when switching between windows in native tabs mode.
    //
    // This is only done on subsequent activations (not the first) to ensure the initial focus
    // path is properly established. Without this guard, the focus state would remain unset until
    // the first mouse click, causing keybindings to be non-functional.
    if selector == sel!(windowDidBecomeKey:) && is_active {
        let window_state = unsafe { get_window_state(this) };
        let mut lock = window_state.lock();

        if lock.activated_least_once {
            if let Some(mut callback) = lock.request_frame_callback.take() {
                lock.renderer.set_presents_with_transaction(true);
                lock.stop_display_link();
                drop(lock);
                callback(RequestFrameOptions::default());

                let mut lock = window_state.lock();
                lock.request_frame_callback = Some(callback);
                lock.renderer.set_presents_with_transaction(false);
                lock.start_display_link();
            }
        } else {
            lock.activated_least_once = true;
        }
    }

    executor
        .spawn(async move {
            let mut lock = window_state.as_ref().lock();
            if is_active {
                lock.move_traffic_light();
            }

            if let Some(mut callback) = lock.activate_callback.take() {
                drop(lock);
                callback(is_active);
                window_state.lock().activate_callback = Some(callback);
            };
        })
        .detach();
}

fn window_should_close(this: &AnyObject, _: id) -> bool {
    let window_state = unsafe { get_window_state(this) };
    let mut lock = window_state.as_ref().lock();
    if let Some(mut callback) = lock.should_close_callback.take() {
        drop(lock);
        let should_close = callback();
        window_state.lock().should_close_callback = Some(callback);
        should_close
    } else {
        true
    }
}

fn close_window(this: &AnyObject) {
    unsafe {
        let (close_callback, simple_fullscreen_state) = {
            let window_state = get_window_state(this);
            let mut lock = window_state.as_ref().lock();
            lock.closed.store(true, Ordering::Release);
            (lock.close_callback.take(), lock.simple_fullscreen_state.take())
        };

        if simple_fullscreen_state.is_some() {
            pop_simple_fullscreen_presentation_options();
        }

        if let Some(callback) = close_callback {
            callback();
        }

        let _: () = msg_send![super(this, class!(NSWindow)), close];
    }
}

fn make_backing_layer(this: &AnyObject) -> id {
    let window_state = unsafe { get_window_state(this) };
    let window_state = window_state.as_ref().lock();
    window_state.renderer.layer_ptr() as id
}

fn view_did_change_backing_properties(this: &AnyObject) {
    let window_state = unsafe { get_window_state(this) };
    update_window_scale_factor(&window_state);
}

fn set_frame_size(this: &AnyObject, size: NSSize) {
    const fn convert(value: NSSize) -> Size<Pixels> {
        Size { width: px(value.width as f32), height: px(value.height as f32) }
    }

    let window_state = unsafe { get_window_state(this) };
    let mut lock = window_state.as_ref().lock();

    let new_size = convert(size);
    let old_size = unsafe {
        let old_frame: NSRect = msg_send![this, frame];
        convert(old_frame.size)
    };

    if old_size == new_size {
        return;
    }

    unsafe {
        let _: () = msg_send![super(this, class!(NSView)), setFrameSize: size];
    }

    let scale_factor = lock.scale_factor();
    let drawable_size = new_size.to_device_pixels(scale_factor);
    lock.renderer.update_drawable_size(drawable_size);

    if let Some(mut callback) = lock.resize_callback.take() {
        let content_size = lock.content_size();
        let scale_factor = lock.scale_factor();
        drop(lock);
        callback(content_size, scale_factor);
        window_state.lock().resize_callback = Some(callback);
    };
}

fn display_layer(this: &AnyObject, _: id) {
    let window_state = unsafe { get_window_state(this) };
    let mut lock = window_state.lock();
    if let Some(mut callback) = lock.request_frame_callback.take() {
        lock.renderer.set_presents_with_transaction(true);
        lock.stop_display_link();
        drop(lock);
        callback(RequestFrameOptions::default());

        let mut lock = window_state.lock();
        lock.request_frame_callback = Some(callback);
        lock.renderer.set_presents_with_transaction(false);
        lock.start_display_link();
    }
}

fn step(view: &AnyObject) {
    let window_state = unsafe { get_window_state(view) };
    let mut lock = window_state.lock();

    if let Some(mut callback) = lock.request_frame_callback.take() {
        drop(lock);
        callback(RequestFrameOptions::default());
        window_state.lock().request_frame_callback = Some(callback);
    }
}

fn valid_attributes_for_marked_text(_: &AnyObject) -> id {
    unsafe { msg_send![class!(NSArray), array] }
}

fn has_marked_text(this: &AnyObject) -> bool {
    let has_marked_text_result = with_input_handler(this, |input_handler| input_handler.marked_text_range()).flatten();

    has_marked_text_result.is_some()
}

fn marked_range(this: &AnyObject) -> NSRange {
    let marked_range_result = with_input_handler(this, |input_handler| input_handler.marked_text_range()).flatten();

    marked_range_result.map_or(NSRange::invalid(), |range| range.into())
}

fn selected_range(this: &AnyObject) -> NSRange {
    let selected_range_result =
        with_input_handler(this, |input_handler| input_handler.selected_text_range(false)).flatten();

    selected_range_result.map_or(NSRange::invalid(), |selection| selection.range.into())
}

fn first_rect_for_character_range(this: &AnyObject, range: NSRange, _: *mut NSRange) -> NSRect {
    let frame = get_frame(this);
    with_input_handler(this, |input_handler| input_handler.bounds_for_range(range.to_range()?)).flatten().map_or(
        NSRect::new(NSPoint::new(0., 0.), NSSize::new(0., 0.)),
        |bounds| {
            NSRect::new(
                NSPoint::new(
                    frame.origin.x + bounds.origin.x.as_f32() as f64,
                    frame.origin.y + frame.size.height
                        - bounds.origin.y.as_f32() as f64
                        - bounds.size.height.as_f32() as f64,
                ),
                NSSize::new(bounds.size.width.as_f32() as f64, bounds.size.height.as_f32() as f64),
            )
        },
    )
}

fn get_frame(this: &AnyObject) -> NSRect {
    unsafe {
        let state = get_window_state(this);
        let lock = state.lock();
        let mut frame: NSRect = msg_send![lock.native_window, frame];
        let content_layout_rect: NSRect = msg_send![lock.native_window, contentLayoutRect];
        let style_mask: NSWindowStyleMask = msg_send![lock.native_window, styleMask];
        if !style_mask.contains(NSWindowStyleMask::FullSizeContentView) {
            frame.origin.y -= frame.size.height - content_layout_rect.size.height;
        }
        frame
    }
}

fn insert_text(this: &AnyObject, text: id, replacement_range: NSRange) {
    unsafe {
        let is_attributed_string: bool = msg_send![text, isKindOfClass: class!(NSAttributedString)];
        let text: id = if is_attributed_string { msg_send![text, string] } else { text };

        let text = text.to_str();
        let replacement_range = replacement_range.to_range();
        with_input_handler(this, |input_handler| input_handler.replace_text_in_range(replacement_range, text));
    }
}

fn set_marked_text(this: &AnyObject, text: id, selected_range: NSRange, replacement_range: NSRange) {
    unsafe {
        let is_attributed_string: bool = msg_send![text, isKindOfClass: class!(NSAttributedString)];
        let text: id = if is_attributed_string { msg_send![text, string] } else { text };
        let selected_range = selected_range.to_range();
        let replacement_range = replacement_range.to_range();
        let text = text.to_str();
        with_input_handler(this, |input_handler| {
            input_handler.replace_and_mark_text_in_range(replacement_range, text, selected_range)
        });
    }
}
fn unmark_text(this: &AnyObject) {
    with_input_handler(this, |input_handler| input_handler.unmark_text());
}

fn attributed_substring_for_proposed_range(this: &AnyObject, range: NSRange, actual_range: *mut NSRange) -> id {
    with_input_handler(this, |input_handler| {
        let range = range.to_range()?;
        if range.is_empty() {
            return None;
        }
        let mut adjusted: Option<Range<usize>> = None;

        let selected_text = input_handler.text_for_range(range.clone(), &mut adjusted)?;
        if let Some(adjusted) = adjusted
            && adjusted != range
        {
            unsafe { actual_range.write(NSRange::from(adjusted)) };
        }
        unsafe {
            let string: id = msg_send![class!(NSAttributedString), alloc];
            let string: id = msg_send![string, initWithString: ns_string(&selected_text)];
            Some(string)
        }
    })
    .flatten()
    .unwrap_or(nil)
}

// We ignore which selector it asks us to do because the user may have
// bound the shortcut to something else.
fn do_command_by_selector(this: &AnyObject, _: Sel) {
    let state = unsafe { get_window_state(this) };
    let mut lock = state.as_ref().lock();
    let keystroke = lock.keystroke_for_do_command.take();
    let mut event_callback = lock.event_callback.take();
    drop(lock);

    if let Some((keystroke, callback)) = keystroke.zip(event_callback.as_mut()) {
        let handled = (callback)(PlatformInput::KeyDown(KeyDownEvent {
            keystroke,
            is_held: false,
            prefer_character_input: false,
        }));
        state.as_ref().lock().do_command_handled = Some(!handled.propagate);
    }

    state.as_ref().lock().event_callback = event_callback;
}

fn view_did_change_effective_appearance(this: &AnyObject) {
    unsafe {
        let state = get_window_state(this);
        let appearance_changed_callback = {
            let mut lock = state.as_ref().lock();
            lock.appearance_changed_callback.take()
        };

        if let Some(mut callback) = appearance_changed_callback {
            callback();
            state.lock().appearance_changed_callback = Some(callback);
        }

        // AppKit can relayout the standard traffic light buttons as part of
        // applying a new appearance, so reapply GPUI's custom position.
        state.lock().move_traffic_light();
    }
}

fn accepts_first_mouse(this: &AnyObject, _: id) -> bool {
    let window_state = unsafe { get_window_state(this) };
    let mut lock = window_state.as_ref().lock();
    lock.first_mouse = true;
    true
}

// Reports which region of the view AppKit should treat as app-owned titlebar content
// (rather than a system-owned window-move region). When `app_owns_titlebar_drag` is
// true, we claim the entire view so AppKit neither drags the window from the titlebar
// nor waits to disambiguate double-clicks before delivering titlebar clicks (the macOS
// 27 delay); such windows implement dragging themselves via [`Window::start_window_move`].
// Otherwise we return an empty rect so AppKit's native titlebar dragging keeps working.
// This is independent of `NSWindow.isMovable`, so the Window-menu tiling items stay
// enabled regardless.
fn opaque_rect_for_window_move_when_in_titlebar(this: &AnyObject) -> NSRect {
    let zero_rect = NSRect::new(NSPoint::new(0., 0.), NSSize::new(0., 0.));
    let window_state = unsafe { get_window_state(this) };
    let app_owns_titlebar_drag = window_state.as_ref().lock().app_owns_titlebar_drag;
    if app_owns_titlebar_drag { unsafe { msg_send![this, bounds] } } else { zero_rect }
}

fn character_index_for_point(this: &AnyObject, position: NSPoint) -> u64 {
    let position = screen_point_to_gpui_point(this, position);
    with_input_handler(this, |input_handler| input_handler.character_index_for_point(position))
        .flatten()
        .map(|index| index as u64)
        .unwrap_or(NSNotFound as u64)
}

fn screen_point_to_gpui_point(this: &AnyObject, position: NSPoint) -> Point<Pixels> {
    let frame = get_frame(this);
    let window_x = position.x - frame.origin.x;
    let window_y = frame.size.height - (position.y - frame.origin.y);

    point(px(window_x as f32), px(window_y as f32))
}

fn is_drag_from_this_window(this: &AnyObject, dragging_info: id) -> bool {
    let source: id = unsafe { msg_send![dragging_info, draggingSource] };
    std::ptr::eq(source as *const AnyObject, this as *const AnyObject)
}

fn dragging_entered(this: &AnyObject, dragging_info: id) -> NSDragOperation {
    let is_source_window = is_drag_from_this_window(this, dragging_info);
    let window_state = unsafe { get_window_state(this) };
    let position = drag_event_position(&window_state, dragging_info);
    let paths = external_paths_from_event(dragging_info);
    if let Some(event) = paths.map(|paths| FileDropEvent::Entered { position, paths })
        && send_file_drop_event(window_state, event)
    {
        if is_source_window {
            return NSDragOperationMove;
        }
        return NSDragOperationCopy;
    }
    NSDragOperationNone
}

fn dragging_updated(this: &AnyObject, dragging_info: id) -> NSDragOperation {
    let is_source_window = is_drag_from_this_window(this, dragging_info);
    let window_state = unsafe { get_window_state(this) };
    let position = drag_event_position(&window_state, dragging_info);
    if send_file_drop_event(window_state, FileDropEvent::Pending { position }) {
        if is_source_window { NSDragOperationMove } else { NSDragOperationCopy }
    } else {
        NSDragOperationNone
    }
}

fn dragging_exited(this: &AnyObject, _: id) {
    let window_state = unsafe { get_window_state(this) };
    send_file_drop_event(window_state, FileDropEvent::Exited);
}

fn perform_drag_operation(this: &AnyObject, dragging_info: id) -> bool {
    let window_state = unsafe { get_window_state(this) };
    let position = drag_event_position(&window_state, dragging_info);
    send_file_drop_event(window_state, FileDropEvent::Submit { position })
}

fn external_paths_from_event(dragging_info: *mut AnyObject) -> Option<ExternalPaths> {
    let mut paths = SmallVec::new();
    let pasteboard: id = unsafe { msg_send![dragging_info, draggingPasteboard] };
    let filenames: id = unsafe { msg_send![pasteboard, propertyListForType: NSFilenamesPboardType] };
    if filenames == nil {
        return None;
    }
    for file in unsafe { array_iter(filenames) } {
        let path = unsafe {
            let f: *const c_char = msg_send![file, UTF8String];
            CStr::from_ptr(f).to_string_lossy().into_owned()
        };
        paths.push(PathBuf::from(path))
    }
    Some(ExternalPaths(paths))
}

fn conclude_drag_operation(this: &AnyObject, _: id) {
    let window_state = unsafe { get_window_state(this) };
    send_file_drop_event(window_state, FileDropEvent::Exited);
}

fn dragging_session_source_operation_mask(_: &AnyObject, _: id, context: NSInteger) -> NSDragOperation {
    let operation = match context {
        NSDRAGGING_CONTEXT_OUTSIDE_APPLICATION => NSDragOperationCopy,
        NSDRAGGING_CONTEXT_WITHIN_APPLICATION => NSDragOperationCopy | NSDragOperationMove,
        _ => NSDragOperationCopy | NSDragOperationMove,
    };
    log::debug!("dragging_session_source_operation_mask: context={}, operation={}", context, operation);
    operation
}

fn dragging_session_ended(this: &AnyObject, _: id, _: NSPoint, operation: NSDragOperation) {
    log::debug!("dragging_session_ended operation={operation}");
    // SAFETY: AppKit invokes this selector on the GPUIWindow or GPUIPanel instance,
    // which always has its window-state ivar initialized to the owning MacWindowState.
    let window_state = unsafe { get_window_state(this) };
    {
        let mut lock = window_state.lock();
        lock.synthetic_drag_counter += 1;
        lock.last_left_mouse_down_event = None;
    }
    send_file_drop_event(window_state, FileDropEvent::Ended);
}

const fn synthetic_drag_button_is_pressed(button: Option<MouseButton>, pressed: NSUInteger) -> bool {
    let bit = match button {
        Some(MouseButton::Left) => 0,
        Some(MouseButton::Right) => 1,
        Some(MouseButton::Middle) => 2,
        Some(MouseButton::Navigate(gpui::NavigationDirection::Back)) => 3,
        Some(MouseButton::Navigate(gpui::NavigationDirection::Forward)) => 4,
        None => return false,
    };
    pressed & (1 << bit) != 0
}

async fn synthetic_drag(
    window_state: Weak<Mutex<MacWindowState>>,
    drag_id: usize,
    event: MouseMoveEvent,
    executor: BackgroundExecutor,
) {
    loop {
        executor.timer(Duration::from_millis(16)).await;
        let Some(window_state) = window_state.upgrade() else {
            break;
        };
        let mut lock = window_state.lock();
        if lock.synthetic_drag_counter != drag_id {
            break;
        }
        // Native menu tracking can consume mouse-up, leaving stale drag events replaying.
        // Check whether the original mouse button is still physically pressed before replaying.
        let pressed: NSUInteger = unsafe { msg_send![class!(NSEvent), pressedMouseButtons] };
        if !synthetic_drag_button_is_pressed(event.pressed_button, pressed) {
            lock.synthetic_drag_counter += 1;
            break;
        }
        if let Some(mut callback) = lock.event_callback.take() {
            drop(lock);
            callback(PlatformInput::MouseMove(event.clone()));
            window_state.lock().event_callback = Some(callback);
        }
    }
}

/// Sends the specified FileDropEvent using `PlatformInput::FileDrop` to the window
/// state and updates the window state according to the event passed.
fn send_file_drop_event(window_state: Arc<Mutex<MacWindowState>>, file_drop_event: FileDropEvent) -> bool {
    let external_files_dragged = match file_drop_event {
        FileDropEvent::Entered { .. } => Some(true),
        FileDropEvent::Exited | FileDropEvent::Ended => Some(false),
        _ => None,
    };

    let mut lock = window_state.lock();
    if let Some(mut callback) = lock.event_callback.take() {
        drop(lock);
        callback(PlatformInput::FileDrop(file_drop_event));
        let mut lock = window_state.lock();
        lock.event_callback = Some(callback);
        if let Some(external_files_dragged) = external_files_dragged {
            lock.external_files_dragged = external_files_dragged;
        }
        true
    } else {
        false
    }
}

fn drag_event_position(window_state: &Mutex<MacWindowState>, dragging_info: id) -> Point<Pixels> {
    let drag_location: NSPoint = unsafe { msg_send![dragging_info, draggingLocation] };
    convert_mouse_position(drag_location, window_state.lock().content_size().height)
}

fn with_input_handler<F, R>(window: &AnyObject, f: F) -> Option<R>
where
    F: FnOnce(&mut PlatformInputHandler) -> R,
{
    let window_state = unsafe { get_window_state(window) };
    let mut lock = window_state.as_ref().lock();
    if let Some(mut input_handler) = lock.input_handler.take() {
        drop(lock);
        let result = f(&mut input_handler);
        window_state.lock().input_handler = Some(input_handler);
        Some(result)
    } else {
        None
    }
}

fn display_id_for_screen(screen: id) -> Option<CGDirectDisplayID> {
    if screen.is_null() {
        return None;
    }

    unsafe {
        let device_description: id = msg_send![screen, deviceDescription];
        let screen_number_key: id = ns_string("NSScreenNumber");
        let screen_number: id = msg_send![device_description, objectForKey: screen_number_key];
        let screen_number: NSUInteger = msg_send![screen_number, unsignedIntegerValue];
        Some(screen_number as CGDirectDisplayID)
    }
}

fn blurred_view_init_with_frame(this: &AnyObject, frame: NSRect) -> id {
    unsafe {
        let view: id = msg_send![super(this, class!(NSVisualEffectView)), initWithFrame: frame];
        // A window-background material, whose tint and extra saturation
        // `blurred_view_update_layer` strips, leaving only the blur. `Selection`, used before,
        // has no `CABackdropLayer` on macOS 27, so nothing behind the window was blurred. (Setting
        // a material also avoids the deprecated default, `AppearanceBased`.)
        let _: () = msg_send![view, setMaterial: NSVisualEffectMaterial::UnderWindowBackground];
        // The default, but blurring what's behind the window is this view's whole purpose.
        let _: () = msg_send![view, setBlendingMode: NSVisualEffectBlendingMode::BehindWindow];
        let _: () = msg_send![view, setState: NSVisualEffectState::Active];
        view
    }
}

fn blurred_view_update_layer(this: &AnyObject) {
    unsafe {
        let _: () = msg_send![super(this, class!(NSVisualEffectView)), updateLayer];
        let layer: id = msg_send![this, layer];
        if !layer.is_null() {
            remove_layer_background(layer);
        }
    }
}

/// Opaque `CGColor`, so a null color is sent with the `^{CGColor=}` encoding AppKit expects.
#[repr(C)]
struct CGColor {
    _private: [u8; 0],
}

unsafe impl objc2::RefEncode for CGColor {
    const ENCODING_REF: objc2::Encoding = objc2::Encoding::Pointer(&objc2::Encoding::Struct("CGColor", &[]));
}

unsafe fn remove_layer_background(layer: id) {
    unsafe {
        let _: () = msg_send![layer, setBackgroundColor: std::ptr::null::<CGColor>()];

        let class_name: id = msg_send![layer, className];
        if class_name.to_str() == "CAChameleonLayer" {
            // Remove the desktop tinting effect.
            let _: () = msg_send![layer, setHidden: true];
            return;
        }

        let filters: id = msg_send![layer, filters];
        if !filters.is_null() {
            // Remove the increased saturation.
            // The effect of a `CAFilter` or `CIFilter` is determined by its name, and the
            // `description` reflects its name and some parameters. Currently `NSVisualEffectView`
            // uses a `CAFilter` named "colorSaturate". If one day they switch to `CIFilter`, the
            // `description` will still contain "Saturat" ("... inputSaturation = ...").
            let test_string: id = ns_string("Saturat");
            let count = array_count(filters);
            for (i, filter) in array_iter(filters).enumerate() {
                let description: id = msg_send![filter, description];
                let hit: bool = msg_send![description, containsString: test_string];
                if !hit {
                    continue;
                }

                let all_indices = NSRange { location: 0, length: count };
                let indices: id = msg_send![class!(NSMutableIndexSet), indexSet];
                let _: () = msg_send![indices, addIndexesInRange: all_indices];
                let _: () = msg_send![indices, removeIndex:i];
                let filtered: id = msg_send![filters, objectsAtIndexes: indices];
                let _: () = msg_send![layer, setFilters: filtered];
                break;
            }
        }

        let sublayers: id = msg_send![layer, sublayers];
        if !sublayers.is_null() {
            for sublayer in array_iter(sublayers) {
                remove_layer_background(sublayer);
            }
        }
    }
}

fn add_titlebar_accessory_view_controller(this: &AnyObject, view_controller: id) {
    unsafe {
        let _: () = msg_send![super(this, class!(NSWindow)), addTitlebarAccessoryViewController: view_controller];

        // Hide the native tab bar and set its height to 0, since we render our own.
        let accessory_view: id = msg_send![view_controller, view];
        let _: () = msg_send![accessory_view, setHidden: true];
        let mut frame: NSRect = msg_send![accessory_view, frame];
        frame.size.height = 0.0;
        let _: () = msg_send![accessory_view, setFrame: frame];
    }
}
