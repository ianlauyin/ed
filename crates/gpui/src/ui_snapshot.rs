//! Opt-in record of what each frame painted, read by external automation (`edctl`).
//!
//! Recording happens in the paint phase, so cached views replay their entries
//! together with the rest of their paint output.

use anyhow::Result;
use image::RgbaImage;

use crate::App;
use crate::Bounds;
use crate::FocusHandle;
use crate::FocusId;
use crate::GlobalElementId;
use crate::Pixels;
use crate::SharedString;
use crate::Window;
use crate::px;

const MIN_READABLE_TEXT_HEIGHT: Pixels = px(6.);

/// An element with an id that takes mouse input or focus, as painted in the last frame.
#[derive(Clone, Debug)]
pub struct UiElement {
    /// The element's full id path.
    pub id: GlobalElementId,
    /// The visible part of the element, clipped by its content mask.
    pub bounds: Bounds<Pixels>,
    /// Whether the element has click listeners.
    pub clickable: bool,
    /// The focus handle the element tracks, if any.
    pub focus: Option<FocusId>,
}

/// A line of text painted in the last frame.
#[derive(Clone, Debug)]
pub struct UiText {
    /// The id path of the elements the text was painted under.
    pub id: GlobalElementId,
    /// The painted text.
    pub text: SharedString,
    /// The visible part of the line, clipped by its content mask.
    pub bounds: Bounds<Pixels>,
}

impl App {
    /// Start recording [`UiElement`]s and [`UiText`]s for every window frame.
    pub fn enable_ui_snapshots(&mut self) {
        self.ui_snapshots = true;
        for window in self.windows() {
            window.update(self, |_, window, _| window.refresh()).ok();
        }
    }

    /// Whether [`App::enable_ui_snapshots`] was called.
    pub const fn ui_snapshots_enabled(&self) -> bool {
        self.ui_snapshots
    }
}

impl Window {
    /// Elements recorded in the last frame, in paint order.
    pub fn ui_elements(&self) -> &[UiElement] {
        &self.rendered_frame.ui_elements
    }

    /// Text recorded in the last frame, in paint order.
    pub fn ui_texts(&self) -> &[UiText] {
        &self.rendered_frame.ui_texts
    }

    /// The id of the focused element's focus handle.
    pub const fn focus_id(&self) -> Option<FocusId> {
        self.focus
    }

    /// Whether the window changed since its last frame was drawn.
    pub fn is_dirty(&self) -> bool {
        self.invalidator.is_dirty()
    }

    /// Draw a new frame now if the window is dirty. The platform presents it on its next frame.
    pub fn draw_if_dirty(&mut self, cx: &mut App) {
        if self.is_dirty() {
            self.draw(cx).clear(cx);
        }
    }

    /// Render the last frame to an image at device resolution.
    pub fn capture_image(&self) -> Result<RgbaImage> {
        self.platform_window.render_to_image(&self.rendered_frame.scene)
    }

    /// Record an element for [`Window::ui_elements`], for custom elements that
    /// take input. Call during paint, after checking [`App::ui_snapshots_enabled`].
    pub fn record_ui_element(
        &mut self,
        id: &GlobalElementId,
        bounds: Bounds<Pixels>,
        clickable: bool,
        focus: Option<&FocusHandle>,
    ) {
        self.invalidator.debug_assert_paint();
        let bounds = bounds.intersect(&self.content_mask().bounds);
        if bounds.is_empty() {
            return;
        }
        let focus = focus.map(|handle| handle.id);
        self.next_frame.ui_elements.push(UiElement { id: id.clone(), bounds, clickable, focus });
    }

    pub(crate) fn record_ui_text(&mut self, text: &SharedString, bounds: Bounds<Pixels>) {
        let bounds = bounds.intersect(&self.content_mask().bounds);
        // Text this small (e.g. the editor minimap) is not readable, so it is not content.
        if bounds.is_empty() || bounds.size.height < MIN_READABLE_TEXT_HEIGHT || text.trim().is_empty() {
            return;
        }
        let id = self.element_id_stack.global_id();
        self.next_frame.ui_texts.push(UiText { id, text: text.clone(), bounds });
    }
}
