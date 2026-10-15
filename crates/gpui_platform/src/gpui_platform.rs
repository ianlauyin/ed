//! GPUI application and renderer construction for macOS.

#[cfg(not(target_os = "macos"))]
compile_error!("This editor only supports macOS.");

pub use gpui::Platform;
use std::rc::Rc;

/// Returns a background executor for macOS.
pub fn background_executor() -> gpui::BackgroundExecutor {
    current_platform(true).background_executor()
}

pub fn application() -> gpui::Application {
    gpui::Application::with_platform(current_platform(false))
}

pub fn headless() -> gpui::Application {
    gpui::Application::with_platform(current_platform(true))
}

/// Returns the macOS platform.
pub fn current_platform(headless: bool) -> Rc<dyn Platform> {
    Rc::new(gpui_macos::MacPlatform::new(headless))
}
