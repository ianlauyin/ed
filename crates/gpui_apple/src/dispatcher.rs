use std::ffi::c_void;
use std::ptr::NonNull;
use std::time::Duration;

use async_task::Runnable;
use dispatch2::DispatchQueue;
use dispatch2::DispatchQueueGlobalPriority;
use dispatch2::DispatchTime;
use dispatch2::GlobalQueueIdentifier;
use gpui::ActivityGuard;
use gpui::PlatformDispatcher;
use gpui::Priority;
use gpui::RunnableMeta;
use gpui::RunnableVariant;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_foundation::NSActivityOptions;
use objc2_foundation::NSObjectProtocol;
use objc2_foundation::NSProcessInfo;
use objc2_foundation::NSString;
use objc2_foundation::NSThread;

pub struct AppleDispatcher;

impl AppleDispatcher {
    pub const fn new() -> Self {
        Self
    }
}

impl Default for AppleDispatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl PlatformDispatcher for AppleDispatcher {
    fn is_main_thread(&self) -> bool {
        NSThread::isMainThread_class()
    }

    fn dispatch(&self, runnable: RunnableVariant, priority: Priority) {
        let context = runnable.into_raw().as_ptr() as *mut c_void;

        let queue_priority = match priority {
            Priority::High => DispatchQueueGlobalPriority::High,
            Priority::Medium => DispatchQueueGlobalPriority::Default,
            Priority::Low => DispatchQueueGlobalPriority::Low,
        };

        unsafe {
            DispatchQueue::global_queue(GlobalQueueIdentifier::Priority(queue_priority))
                .exec_async_f(context, trampoline);
        }
    }

    fn dispatch_on_main_thread(&self, runnable: RunnableVariant, _priority: Priority) {
        let context = runnable.into_raw().as_ptr() as *mut c_void;
        unsafe {
            DispatchQueue::main().exec_async_f(context, trampoline);
        }
    }

    fn dispatch_after(&self, duration: Duration, runnable: RunnableVariant) {
        let context = runnable.into_raw().as_ptr() as *mut c_void;
        let queue = DispatchQueue::global_queue(GlobalQueueIdentifier::Priority(DispatchQueueGlobalPriority::High));
        let when = DispatchTime::NOW.time(duration.as_nanos() as i64);
        unsafe {
            DispatchQueue::exec_after_f(when, &queue, context, trampoline);
        }
    }

    fn prevent_app_nap(&self, reason: &str) -> ActivityGuard {
        AppleActivity::begin(reason, NSActivityOptions::UserInitiatedAllowingIdleSystemSleep)
    }
}

pub struct AppleActivity {
    activity: Retained<ProtocolObject<dyn NSObjectProtocol>>,
}

// The activity token returned by NSProcessInfo is thread-safe
unsafe impl Send for AppleActivity {}

impl AppleActivity {
    pub fn begin(reason: &str, options: NSActivityOptions) -> ActivityGuard {
        let activity = Self {
            activity: NSProcessInfo::processInfo()
                .beginActivityWithOptions_reason(options, &NSString::from_str(reason)),
        };
        ActivityGuard::new(move || drop(activity))
    }
}

impl Drop for AppleActivity {
    fn drop(&mut self) {
        unsafe { NSProcessInfo::processInfo().endActivity(&self.activity) };
    }
}

extern "C" fn trampoline(context: *mut c_void) {
    let runnable = unsafe { Runnable::<RunnableMeta>::from_raw(NonNull::new_unchecked(context as *mut ())) };

    runnable.run();
}
