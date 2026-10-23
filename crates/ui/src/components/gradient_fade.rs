use gpui::Hsla;
use gpui::Pixels;
use gpui::SharedString;
use gpui::linear_color_stop;
use gpui::linear_gradient;
use gpui::px;

use crate::prelude::*;

/// A gradient overlay that fades from a solid color to transparent.
#[derive(IntoElement)]
pub struct GradientFade {
    base_bg: Hsla,
    hover_bg: Hsla,
    active_bg: Hsla,
    width: Pixels,
    width_hovered: Option<Pixels>,
    right: Pixels,
    gradient_stop: f32,
    group_name: Option<SharedString>,
}

impl GradientFade {
    pub const fn new(base_bg: Hsla, hover_bg: Hsla, active_bg: Hsla) -> Self {
        Self {
            base_bg,
            hover_bg,
            active_bg,
            width: px(48.0),
            right: px(0.0),
            gradient_stop: 0.6,
            group_name: None,
            width_hovered: None,
        }
    }

    pub const fn width(mut self, width: Pixels) -> Self {
        self.width = width;
        self
    }

    pub const fn width_hovered(mut self, width: Pixels) -> Self {
        self.width_hovered = Some(width);
        self
    }

    pub const fn right(mut self, right: Pixels) -> Self {
        self.right = right;
        self
    }

    pub const fn gradient_stop(mut self, stop: f32) -> Self {
        self.gradient_stop = stop;
        self
    }

    pub fn group_name(mut self, name: impl Into<SharedString>) -> Self {
        self.group_name = Some(name.into());
        self
    }
}

impl RenderOnce for GradientFade {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let stop = self.gradient_stop;

        // Best-effort to flatten potentially-transparent colors to opaque ones.
        let app_bg = cx.theme().colors().background;
        let base_bg = app_bg.blend(self.base_bg);
        let hover_bg = app_bg.blend(self.hover_bg);
        let active_bg = app_bg.blend(self.active_bg);

        div()
            .id("gradient_fade")
            .absolute()
            .top_0()
            .right(self.right)
            .w(self.width)
            .h_full()
            .bg(linear_gradient(90., linear_color_stop(base_bg, stop), linear_color_stop(base_bg.opacity(0.0), 0.)))
            .when_some(self.group_name.clone(), |element, group_name| {
                element.group_hover(group_name, move |s| {
                    s.bg(linear_gradient(
                        90.,
                        linear_color_stop(hover_bg, stop),
                        linear_color_stop(hover_bg.opacity(0.0), 0.),
                    ))
                    .w(self.width_hovered.unwrap_or(self.width))
                })
            })
            .when_some(self.group_name, |element, group_name| {
                element.group_active(group_name, move |s| {
                    s.bg(linear_gradient(
                        90.,
                        linear_color_stop(active_bg, stop),
                        linear_color_stop(active_bg.opacity(0.0), 0.),
                    ))
                })
            })
    }
}
