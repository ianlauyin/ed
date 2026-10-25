//! Native macOS applications and the headless Linux remote server.

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
compile_error!("Supported targets are macOS and the headless Linux remote server.");

#[cfg(target_os = "linux")]
mod linux;

use std::rc::Rc;

pub use gpui::Platform;

/// Returns a native background executor.
pub fn background_executor() -> gpui::BackgroundExecutor {
    current_platform(true).background_executor()
}

#[cfg(target_os = "macos")]
pub fn application() -> gpui::Application {
    gpui::Application::with_platform(current_platform(false))
}

pub fn headless() -> gpui::Application {
    gpui::Application::with_platform(current_platform(true))
}

/// Returns the native platform. Linux supports headless operation only.
pub fn current_platform(headless: bool) -> Rc<dyn Platform> {
    #[cfg(target_os = "macos")]
    {
        Rc::new(gpui_macos::MacPlatform::new(headless))
    }
    #[cfg(target_os = "linux")]
    {
        assert!(headless, "the Linux remote server only supports headless operation");
        Rc::new(linux::HeadlessPlatform::new())
    }
}
