mod system_window_tabs;

use gpui::{
    AnyElement, App, Context, Entity, Hsla, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, WeakEntity, Window, WindowControlArea,
};
use smallvec::SmallVec;
use std::mem;
use system_window_tabs::SystemWindowTabs;
use ui::{
    prelude::*,
    utils::{TRAFFIC_LIGHT_PADDING, platform_title_bar_height},
};
use workspace::{MultiWorkspace, SidebarRenderState, SidebarSide};

pub use system_window_tabs::{
    DraggedWindowTab, MergeAllWindows, MoveTabToNewWindow, ShowNextWindowTab, ShowPreviousWindowTab,
};

pub struct PlatformTitleBar {
    id: ElementId,
    children: SmallVec<[AnyElement; 2]>,
    should_move: bool,
    system_window_tabs: Entity<SystemWindowTabs>,
    multi_workspace: Option<WeakEntity<MultiWorkspace>>,
}

impl PlatformTitleBar {
    pub fn new(id: impl Into<ElementId>, cx: &mut Context<Self>) -> Self {
        let system_window_tabs = cx.new(|_cx| SystemWindowTabs::new());
        Self {
            id: id.into(),
            children: SmallVec::new(),
            should_move: false,
            system_window_tabs,
            multi_workspace: None,
        }
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

    pub fn init(cx: &mut App) {
        SystemWindowTabs::init(cx);
    }

    fn sidebar_render_state(&self, cx: &App) -> SidebarRenderState {
        self.multi_workspace
            .as_ref()
            .and_then(|mw| mw.upgrade())
            .map(|mw| mw.read(cx).sidebar_render_state(cx))
            .unwrap_or_default()
    }

    pub fn is_multi_workspace_enabled(_cx: &App) -> bool {
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

        let title_bar = h_flex()
            .id(self.id.clone())
            .window_control_area(WindowControlArea::Drag)
            .w_full()
            .h(height)
            .on_mouse_down_out(cx.listener(|this, _, _, _| this.should_move = false))
            .on_mouse_up(
                gpui::MouseButton::Left,
                cx.listener(|this, _, _, _| this.should_move = false),
            )
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, _, _| this.should_move = true),
            )
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
            );

        v_flex()
            .w_full()
            .child(title_bar)
            .child(self.system_window_tabs.clone().into_any_element())
    }
}

impl ParentElement for PlatformTitleBar {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements)
    }
}
