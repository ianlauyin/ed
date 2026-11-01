use gpui::Pixels;
use gpui::Window;
use gpui::px;

pub const MACOS_SDK_26_OR_LATER: bool = cfg!(macos_sdk_26_or_later);

// Use pixels here instead of a rem-based size because the macOS traffic
// lights are a static size, and don't scale with the rest of the UI.
//
/// Where the macOS traffic lights sit: as far in from the left edge as the
/// window's rounded corner needs, and centred in the 36px tab row below the
/// 8px card gap (the buttons are 14px).
pub const TRAFFIC_LIGHT_POSITION: (f32, f32) = (16., 19.);

/// Room from the window's left edge to the end of the traffic lights plus a
/// margin. Tahoe's buttons are spaced wider.
pub const TRAFFIC_LIGHT_PADDING: f32 = TRAFFIC_LIGHT_POSITION.0 + if MACOS_SDK_26_OR_LATER { 69. } else { 62. };

/// Returns the title bar height, which scales with the window's rem size (1.75x) with a
/// minimum of 34px.
pub fn platform_title_bar_height(window: &Window) -> Pixels {
    (1.75 * window.rem_size()).max(px(34.))
}
