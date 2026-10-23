use std::mem;

use gpui::AnyElement;
use gpui::App;
use gpui::Context;
use gpui::Hsla;
use gpui::InteractiveElement;
use gpui::IntoElement;
use gpui::ParentElement;
use gpui::StatefulInteractiveElement;
use gpui::Styled;
use gpui::WeakEntity;
use gpui::Window;
use gpui::WindowControlArea;
use smallvec::SmallVec;
use ui::prelude::*;
use ui::utils::TRAFFIC_LIGHT_PADDING;
use ui::utils::platform_title_bar_height;
use workspace::MultiWorkspace;
use workspace::SidebarRenderState;
use workspace::SidebarSide;

pub struct PlatformTitleBar {
    id: ElementId,
    children: SmallVec<[AnyElement; 2]>,
    should_move: bool,
    multi_workspace: Option<WeakEntity<MultiWorkspace>>,
}

impl PlatformTitleBar {
    pub fn new(id: impl Into<ElementId>, _cx: &mut Context<Self>) -> Self {
        Self { id: id.into(), children: SmallVec::new(), should_move: false, multi_workspace: None }
    }

    pub fn with_multi_workspace(mut self, multi_workspace: WeakEntity<MultiWorkspace>) -> Self {
        self.multi_workspace = Some(multi_workspace);
        self
    }

    pub fn set_multi_workspace(&mut self, multi_workspace: WeakEntity<MultiWorkspace>) {
        self.multi_workspace = Some(multi_workspace);
    }

    pub fn title_bar_color(&self, _window: &mut Window, cx: &mut Context<Self>) -> Hsla {
        cx.theme().colors().title_bar_background
    }

    pub fn set_children<T>(&mut self, children: T)
    where
        T: IntoIterator<Item = AnyElement>,
    {
        self.children = children.into_iter().collect();
    }

    fn sidebar_render_state(&self, cx: &App) -> SidebarRenderState {
        self.multi_workspace
            .as_ref()
            .and_then(|mw| mw.upgrade())
            .map(|mw| mw.read(cx).sidebar_render_state(cx))
            .unwrap_or_default()
    }

    pub const fn is_multi_workspace_enabled(_cx: &App) -> bool {
        false
    }
}

impl Render for PlatformTitleBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let height = platform_title_bar_height(window);
        let titlebar_color = self.title_bar_color(window, cx);
        let children = mem::take(&mut self.children);
        let sidebar = self.sidebar_render_state(cx);
        let show_left_controls = !(sidebar.open && sidebar.side == SidebarSide::Left);

        h_flex()
            .id(self.id.clone())
            .window_control_area(WindowControlArea::Drag)
            .w_full()
            .h(height)
            .on_mouse_down_out(cx.listener(|this, _, _, _| this.should_move = false))
            .on_mouse_up(gpui::MouseButton::Left, cx.listener(|this, _, _, _| this.should_move = false))
            .on_mouse_down(gpui::MouseButton::Left, cx.listener(|this, _, _, _| this.should_move = true))
            .on_mouse_move(cx.listener(|this, _, window, _| {
                if this.should_move {
                    this.should_move = false;
                    window.start_window_move();
                }
            }))
            .on_click(|event, window, _| {
                if event.click_count() == 2 {
                    window.titlebar_double_click();
                }
            })
            .map(|this| {
                if !window.is_fullscreen() && !window.is_simple_fullscreen() && show_left_controls {
                    this.pl(px(TRAFFIC_LIGHT_PADDING))
                } else {
                    this.pl_2()
                }
            })
            .bg(titlebar_color)
            .content_stretch()
            .child(
                div()
                    .id(self.id.clone())
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .overflow_x_hidden()
                    .w_full()
                    .children(children),
            )
    }
}

impl ParentElement for PlatformTitleBar {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements)
    }
}
