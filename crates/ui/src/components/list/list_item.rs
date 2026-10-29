use std::sync::Arc;

use gpui::AnyElement;
use gpui::AnyView;
use gpui::ClickEvent;
use gpui::MouseButton;
use gpui::MouseDownEvent;
use gpui::Pixels;
use gpui::px;
use smallvec::SmallVec;

use crate::Disclosure;
use crate::prelude::*;

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy, Default)]
pub enum ListItemSpacing {
    #[default]
    Dense,
    ExtraDense,
    Sparse,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum DockSide {
    Left,
    Right,
}

#[derive(Default)]
enum EndSlotVisibility {
    #[default]
    Always,
    OnHover,
    SwapOnHover(AnyElement),
}

#[derive(IntoElement)]
pub struct ListItem {
    id: ElementId,
    group_name: Option<SharedString>,
    disabled: bool,
    selected: bool,
    spacing: ListItemSpacing,
    indent_level: usize,
    indent_step_size: Pixels,
    /// A slot for content that appears before the children, like an icon or avatar.
    start_slot: Option<AnyElement>,
    /// A slot for content that appears after the children, usually on the other side of the header.
    /// This might be a button, a disclosure arrow, a face pile, etc.
    end_slot: Option<AnyElement>,
    end_slot_visibility: EndSlotVisibility,
    toggle: Option<bool>,
    inset: bool,
    #[allow(clippy::type_complexity)]
    on_click: Option<Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>>,
    #[allow(clippy::type_complexity)]
    on_hover: Option<Box<dyn Fn(&bool, &mut Window, &mut App) + 'static>>,
    #[allow(clippy::type_complexity)]
    on_toggle: Option<Arc<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>>,
    #[allow(clippy::type_complexity)]
    tooltip: Option<Box<dyn Fn(&mut Window, &mut App) -> AnyView + 'static>>,
    #[allow(clippy::type_complexity)]
    on_secondary_mouse_down: Option<Box<dyn Fn(&MouseDownEvent, &mut Window, &mut App) + 'static>>,
    children: SmallVec<[AnyElement; 2]>,
    selectable: bool,
    always_show_disclosure_icon: bool,
    outlined: bool,
    rounded: bool,
    overflow_x: bool,
    focused: Option<bool>,
    dock: Option<DockSide>,
    height: Option<DefiniteLength>,
}

impl ListItem {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            group_name: None,
            disabled: false,
            selected: false,
            spacing: ListItemSpacing::Dense,
            indent_level: 0,
            indent_step_size: px(12.),
            start_slot: None,
            end_slot: None,
            end_slot_visibility: EndSlotVisibility::default(),
            toggle: None,
            inset: false,
            on_click: None,
            on_secondary_mouse_down: None,
            on_toggle: None,
            on_hover: None,
            tooltip: None,
            children: SmallVec::new(),
            selectable: true,
            always_show_disclosure_icon: false,
            outlined: false,
            rounded: false,
            overflow_x: false,
            focused: None,
            dock: None,
            height: None,
        }
    }

    pub fn group_name(mut self, group_name: impl Into<SharedString>) -> Self {
        self.group_name = Some(group_name.into());
        self
    }

    pub const fn spacing(mut self, spacing: ListItemSpacing) -> Self {
        self.spacing = spacing;
        self
    }

    pub const fn selectable(mut self, has_hover: bool) -> Self {
        self.selectable = has_hover;
        self
    }

    pub const fn always_show_disclosure_icon(mut self, show: bool) -> Self {
        self.always_show_disclosure_icon = show;
        self
    }

    pub fn on_click(mut self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }

    pub fn on_hover(mut self, handler: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_hover = Some(Box::new(handler));
        self
    }

    pub fn on_secondary_mouse_down(
        mut self,
        handler: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_secondary_mouse_down = Some(Box::new(handler));
        self
    }

    pub fn tooltip(mut self, tooltip: impl Fn(&mut Window, &mut App) -> AnyView + 'static) -> Self {
        self.tooltip = Some(Box::new(tooltip));
        self
    }

    pub const fn inset(mut self, inset: bool) -> Self {
        self.inset = inset;
        self
    }

    pub const fn indent_level(mut self, indent_level: usize) -> Self {
        self.indent_level = indent_level;
        self
    }

    pub const fn indent_step_size(mut self, indent_step_size: Pixels) -> Self {
        self.indent_step_size = indent_step_size;
        self
    }

    pub fn toggle(mut self, toggle: impl Into<Option<bool>>) -> Self {
        self.toggle = toggle.into();
        self
    }

    pub fn on_toggle(mut self, on_toggle: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_toggle = Some(Arc::new(on_toggle));
        self
    }

    pub fn start_slot<E: IntoElement>(mut self, start_slot: impl Into<Option<E>>) -> Self {
        self.start_slot = start_slot.into().map(IntoElement::into_any_element);
        self
    }

    pub fn end_slot<E: IntoElement>(mut self, end_slot: impl Into<Option<E>>) -> Self {
        self.end_slot = end_slot.into().map(IntoElement::into_any_element);
        self
    }

    pub fn end_slot_on_hover<E: IntoElement>(mut self, end_slot_on_hover: E) -> Self {
        self.end_slot_visibility = EndSlotVisibility::SwapOnHover(end_slot_on_hover.into_any_element());
        self
    }

    pub fn show_end_slot_on_hover(mut self) -> Self {
        self.end_slot_visibility = EndSlotVisibility::OnHover;
        self
    }

    pub const fn outlined(mut self) -> Self {
        self.outlined = true;
        self
    }

    pub const fn rounded(mut self) -> Self {
        self.rounded = true;
        self
    }

    pub const fn overflow_x(mut self) -> Self {
        self.overflow_x = true;
        self
    }

    pub const fn focused(mut self, focused: bool) -> Self {
        self.focused = Some(focused);
        self
    }

    pub fn dock(mut self, dock: impl Into<Option<DockSide>>) -> Self {
        self.dock = dock.into();
        self
    }

    pub fn height(mut self, height: impl Into<DefiniteLength>) -> Self {
        self.height = Some(height.into());
        self
    }
}

impl Disableable for ListItem {
    fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl Toggleable for ListItem {
    fn toggle_state(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
}

impl ParentElement for ListItem {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements)
    }
}

impl RenderOnce for ListItem {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        h_flex()
            .id(self.id)
            .when_some(self.group_name, |this, group| this.group(group))
            .w_full()
            .when_some(self.height, |this, height| this.h(height))
            .relative()
            // When an item is inset draw the indent spacing outside of the item
            .when(self.inset, |this| {
                this.ml(self.indent_level as f32 * self.indent_step_size).px(DynamicSpacing::Base04.rems(cx))
            })
            .when(!self.inset, |this| {
                this.when_some(self.focused, |this, focused| {
                    this.border_1()
                        .when_some(self.dock, |this, dock| match dock {
                            DockSide::Left => this.border_l_2(),
                            DockSide::Right => this.border_r_2(),
                        })
                        .when(focused && !self.disabled, |this| this.border_color(cx.theme().colors().border_focused))
                })
                .when(self.selectable && !self.disabled, |this| {
                    this.hover(|style| style.bg(cx.theme().colors().ghost_element_hover))
                        .active(|style| style.bg(cx.theme().colors().ghost_element_active))
                        .when(self.outlined, |this| this.rounded_sm())
                        .when(self.selected, |this| this.bg(cx.theme().colors().ghost_element_selected))
                })
            })
            .when(self.rounded, |this| this.rounded_sm())
            .when_some(self.on_hover, |this, on_hover| this.on_hover(on_hover))
            .child(
                h_flex()
                    .id("inner_list_item")
                    .group("list_item")
                    .w_full()
                    .relative()
                    .gap_1()
                    .px(DynamicSpacing::Base06.rems(cx))
                    .map(|this| match self.spacing {
                        ListItemSpacing::Dense => this,
                        ListItemSpacing::ExtraDense => this.py_neg_px(),
                        ListItemSpacing::Sparse => this.py_1(),
                    })
                    .when(self.inset, |this| {
                        this.when_some(self.focused, |this, focused| {
                            if focused && !self.disabled {
                                this.border_1().border_color(cx.theme().colors().border_focused)
                            } else {
                                this.border_1()
                            }
                        })
                        .when(self.selectable && !self.disabled, |this| {
                            this.hover(|style| style.bg(cx.theme().colors().ghost_element_hover))
                                .active(|style| style.bg(cx.theme().colors().ghost_element_active))
                                .when(self.selected, |this| this.bg(cx.theme().colors().ghost_element_selected))
                        })
                    })
                    .when_some(self.on_click.filter(|_| !self.disabled), |this, on_click| {
                        this.cursor_pointer().on_click(on_click)
                    })
                    .when(self.outlined, |this| {
                        this.border_1().border_color(cx.theme().colors().border).rounded_sm().overflow_hidden()
                    })
                    .when_some(self.on_secondary_mouse_down, |this, on_mouse_down| {
                        this.on_mouse_down(MouseButton::Right, move |event, window, cx| {
                            (on_mouse_down)(event, window, cx)
                        })
                    })
                    .when_some(self.tooltip, |this, tooltip| this.tooltip(tooltip))
                    .map(|this| {
                        if self.inset {
                            this.rounded_sm()
                        } else {
                            // When an item is not inset draw the indent spacing inside of the item
                            this.ml(self.indent_level as f32 * self.indent_step_size)
                        }
                    })
                    .children(self.toggle.map(|is_open| {
                        div()
                            .flex()
                            .absolute()
                            .left(rems(-1.))
                            .when(is_open && !self.always_show_disclosure_icon, |this| this.visible_on_hover(""))
                            .child(Disclosure::new("toggle", is_open).on_toggle_expanded(self.on_toggle))
                    }))
                    .child(
                        h_flex()
                            .flex_grow_1()
                            .flex_shrink_0()
                            .flex_basis(relative(0.25))
                            .gap(DynamicSpacing::Base06.rems(cx))
                            .map(
                                |list_content| {
                                    if self.overflow_x { list_content } else { list_content.overflow_hidden() }
                                },
                            )
                            .children(self.start_slot)
                            .children(self.children),
                    )
                    .when(self.end_slot.is_some(), |this| this.justify_between())
                    .map(|this| match self.end_slot_visibility {
                        EndSlotVisibility::Always if let Some(end_slot) = self.end_slot => {
                            this.child(h_flex().flex_shrink_1().overflow_hidden().child(end_slot))
                        }
                        EndSlotVisibility::OnHover if let Some(end_slot) = self.end_slot => this.child(
                            h_flex().flex_shrink_1().overflow_hidden().visible_on_hover("list_item").child(end_slot),
                        ),
                        EndSlotVisibility::SwapOnHover(hover_slot) => this.child(
                            h_flex()
                                .relative()
                                .flex_shrink_1()
                                .child(h_flex().visible_on_hover("list_item").child(hover_slot))
                                .when_some(self.end_slot, |this, end_slot| {
                                    this.child(
                                        h_flex()
                                            .absolute()
                                            .inset_0()
                                            .justify_end()
                                            .overflow_hidden()
                                            .group_hover("list_item", |this| this.invisible())
                                            .child(end_slot),
                                    )
                                }),
                        ),
                        _ => this,
                    }),
            )
    }
}
