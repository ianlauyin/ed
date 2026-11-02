use gpui::AnyView;
use gpui::App;
use gpui::Context;
use gpui::Div;
use gpui::Entity;
use gpui::EntityId;
use gpui::EventEmitter;
use gpui::Global;
use gpui::ParentElement as _;
use gpui::Render;
use gpui::Styled;
use gpui::Window;
use ui::h_flex;
use ui::prelude::*;
use ui::v_flex;

use crate::ItemHandle;

pub struct PaneSearchBarCallbacks {
    pub setup_search_bar: fn(&Entity<Toolbar>, &mut Window, &mut App),
    pub wrap_div_with_search_actions: fn(Div, Entity<crate::Pane>) -> Div,
}

impl Global for PaneSearchBarCallbacks {}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum ToolbarItemEvent {
    ChangeLocation(ToolbarItemLocation),
}

pub trait ToolbarItemView: Render + EventEmitter<ToolbarItemEvent> {
    fn set_active_pane_item(
        &mut self,
        active_pane_item: Option<&dyn crate::ItemHandle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> ToolbarItemLocation;
}

trait ToolbarItemViewHandle: Send {
    fn id(&self) -> EntityId;
    fn to_any(&self) -> AnyView;
    fn set_active_pane_item(
        &self,
        active_pane_item: Option<&dyn ItemHandle>,
        window: &mut Window,
        cx: &mut App,
    ) -> ToolbarItemLocation;
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum ToolbarItemLocation {
    Hidden,
    PrimaryLeft,
    PrimaryRight,
    Secondary,
}

pub struct Toolbar {
    active_item: Option<Box<dyn ItemHandle>>,
    hidden: bool,
    can_navigate: bool,
    items: Vec<(Box<dyn ToolbarItemViewHandle>, ToolbarItemLocation)>,
}

impl Toolbar {
    fn has_any_visible_items(&self) -> bool {
        self.items.iter().any(|(_item, location)| *location != ToolbarItemLocation::Hidden)
    }

    fn left_items(&self) -> impl Iterator<Item = &dyn ToolbarItemViewHandle> {
        self.items.iter().filter_map(|(item, location)| {
            if *location == ToolbarItemLocation::PrimaryLeft { Some(item.as_ref()) } else { None }
        })
    }

    fn right_items(&self) -> impl Iterator<Item = &dyn ToolbarItemViewHandle> {
        self.items.iter().filter_map(|(item, location)| {
            if *location == ToolbarItemLocation::PrimaryRight { Some(item.as_ref()) } else { None }
        })
    }

    fn secondary_items(&self) -> impl Iterator<Item = &dyn ToolbarItemViewHandle> {
        self.items.iter().rev().filter_map(|(item, location)| {
            if *location == ToolbarItemLocation::Secondary { Some(item.as_ref()) } else { None }
        })
    }
}

impl Render for Toolbar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.has_any_visible_items() {
            return div();
        }

        let secondary_items = self.secondary_items().map(|item| item.to_any());

        let has_left_items = self.left_items().count() > 0;
        let has_right_items = self.right_items().count() > 0;

        v_flex()
            .group("toolbar")
            .relative()
            .py(DynamicSpacing::Base06.rems())
            .px(DynamicSpacing::Base08.rems())
            .when(has_left_items || has_right_items, |this| this.gap(DynamicSpacing::Base06.rems()))
            .bg(cx.theme().colors().toolbar_background)
            .when(has_left_items || has_right_items, |this| {
                this.child(
                    h_flex()
                        .items_start()
                        // `justify_between` would put a lone right group on the left.
                        .map(|this| if has_left_items { this.justify_between() } else { this.justify_end() })
                        .gap(DynamicSpacing::Base08.rems())
                        .when(has_left_items, |this| {
                            this.child(
                                h_flex()
                                    .min_h_8()
                                    .flex_auto()
                                    .justify_start()
                                    .overflow_x_hidden()
                                    .children(self.left_items().map(|item| item.to_any())),
                            )
                        })
                        .when(has_right_items, |this| {
                            this.child(
                                h_flex()
                                    .h_8()
                                    .flex_row_reverse()
                                    .when(has_left_items, |this| this.flex_none())
                                    .justify_end()
                                    .children(self.right_items().map(|item| item.to_any())),
                            )
                        }),
                )
            })
            .children(secondary_items)
    }
}

impl Default for Toolbar {
    fn default() -> Self {
        Self::new()
    }
}

impl Toolbar {
    pub fn new() -> Self {
        Self { active_item: None, items: Vec::default(), hidden: false, can_navigate: true }
    }

    pub fn set_can_navigate(&mut self, can_navigate: bool, cx: &mut Context<Self>) {
        self.can_navigate = can_navigate;
        cx.notify();
    }

    pub fn add_item<T>(&mut self, item: Entity<T>, window: &mut Window, cx: &mut Context<Self>)
    where
        T: 'static + ToolbarItemView,
    {
        let location = item.set_active_pane_item(self.active_item.as_deref(), window, cx);
        cx.subscribe(&item, |this, item, event, cx| {
            if let Some((_, current_location)) = this.items.iter_mut().find(|(i, _)| i.id() == item.entity_id()) {
                match event {
                    ToolbarItemEvent::ChangeLocation(new_location) => {
                        if new_location != current_location {
                            *current_location = *new_location;
                            cx.notify();
                        }
                    }
                }
            }
        })
        .detach();
        self.items.push((Box::new(item), location));
        cx.notify();
    }

    pub fn set_active_item(&mut self, item: Option<&dyn ItemHandle>, window: &mut Window, cx: &mut Context<Self>) {
        self.active_item = item.map(|item| item.boxed_clone());
        self.hidden = self.active_item.as_ref().map(|item| !item.show_toolbar(cx)).unwrap_or(false);

        for (toolbar_item, current_location) in &mut self.items {
            let new_location = toolbar_item.set_active_pane_item(item, window, cx);
            if new_location != *current_location {
                *current_location = new_location;
                cx.notify();
            }
        }
    }

    pub fn item_of_type<T: ToolbarItemView>(&self) -> Option<Entity<T>> {
        self.items.iter().find_map(|(item, _)| item.to_any().downcast().ok())
    }

    pub const fn hidden(&self) -> bool {
        self.hidden
    }
}

impl<T: ToolbarItemView> ToolbarItemViewHandle for Entity<T> {
    fn id(&self) -> EntityId {
        self.entity_id()
    }

    fn to_any(&self) -> AnyView {
        self.clone().into()
    }

    fn set_active_pane_item(
        &self,
        active_pane_item: Option<&dyn ItemHandle>,
        window: &mut Window,
        cx: &mut App,
    ) -> ToolbarItemLocation {
        self.update(cx, |this, cx| this.set_active_pane_item(active_pane_item, window, cx))
    }
}
